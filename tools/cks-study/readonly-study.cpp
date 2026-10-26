// Offline analysis harness. Never link the upstream _tmain.cpp or kernel files.
// The only registry access in this program requests KEY_QUERY_VALUE.
#include <windows.h>
#include <winternl.h>
#include <cstdio>
#include <cstring>
#include <iostream>
#include <stdexcept>
#include "ProductPolicyParser.hpp"

static std::vector<uint8_t> ReadPolicy() {
    HKEY key = nullptr;
    auto status = RegOpenKeyExW(HKEY_LOCAL_MACHINE,
        L"SYSTEM\\CurrentControlSet\\Control\\ProductOptions", 0, KEY_QUERY_VALUE, &key);
    if (status != ERROR_SUCCESS) throw std::runtime_error("Cannot open policy for reading");
    DWORD size = 0, type = 0;
    status = RegQueryValueExW(key, L"ProductPolicy", nullptr, &type, nullptr, &size);
    if (status != ERROR_SUCCESS || type != REG_BINARY || size > 4 * 1024 * 1024) {
        RegCloseKey(key);
        throw std::runtime_error("Invalid policy size/type");
    }
    std::vector<uint8_t> bytes(size);
    status = RegQueryValueExW(key, L"ProductPolicy", nullptr, &type, bytes.data(), &size);
    RegCloseKey(key);
    if (status != ERROR_SUCCESS) throw std::runtime_error("Cannot read policy");
    bytes.resize(size);
    return bytes;
}

// Check record boundaries before giving trusted host data to the old parser.
static void Validate(const std::vector<uint8_t>& b) {
    auto u16 = [&](size_t o) { uint16_t v; std::memcpy(&v, b.data()+o, 2); return v; };
    auto u32 = [&](size_t o) { uint32_t v; std::memcpy(&v, b.data()+o, 4); return v; };
    if (b.size() < 24 || u32(0) != b.size() || u32(4) != b.size()-24 ||
        u32(8) != 4 || u32(16) != 1 || u32(b.size()-4) != 0x45)
        throw std::runtime_error("Invalid policy header");
    size_t p = 20, end = b.size()-4;
    while (p < end) {
        if (end-p < 16) throw std::runtime_error("Truncated record");
        size_t total = u16(p), name = u16(p+2), type = u16(p+4), data = u16(p+6);
        if (total < 16 || total > end-p || name % 2 || name > total-16 ||
            data > total-16-name || (type == REG_DWORD && data != 4) ||
            (type == REG_SZ && data % 2)) throw std::runtime_error("Invalid record");
        p += total;
    }
    if (p != end) throw std::runtime_error("Incorrect end boundary");
}

static const char* Lookup(ProductPolicy& policy, const std::wstring& name) {
    try {
        auto& value = policy[name];
        return value.GetName() == name ? "exact" : "WRONG_NAME";
    } catch (const std::out_of_range&) { return "out_of_range"; }
}

static const char* ConstLookup(const ProductPolicy& policy, const std::wstring& name) {
    try {
        const auto& value = policy[name];
        return value.GetName() == name ? "exact" : "WRONG_NAME";
    } catch (const std::out_of_range&) { return "out_of_range"; }
}

static std::vector<uint8_t> Fixture() {
    // Three valid, sorted DWORD records; independent of upstream serialization.
    std::vector<uint8_t> b(24 + 3 * 24, 0);
    auto put16 = [&](size_t p, uint16_t v) { std::memcpy(b.data()+p, &v, 2); };
    auto put32 = [&](size_t p, uint32_t v) { std::memcpy(b.data()+p, &v, 4); };
    put32(0, static_cast<uint32_t>(b.size())); put32(4, 72); put32(8, 4); put32(16, 1);
    for (size_t i = 0; i < 3; ++i) {
        size_t p = 20 + i * 24;
        put16(p, 24); put16(p+2, 2); put16(p+4, REG_DWORD); put16(p+6, 4);
        put16(p+16, static_cast<uint16_t>(L'A'+i)); put32(p+18, 0);
    }
    put32(b.size()-4, 0x45);
    return b;
}

static void LicenseQuery(const wchar_t* name) {
    using Query = LONG (NTAPI*)(PUNICODE_STRING, PULONG, PVOID, ULONG, PULONG);
    auto query = reinterpret_cast<Query>(GetProcAddress(GetModuleHandleW(L"ntdll.dll"), "NtQueryLicenseValue"));
    if (!query) { std::cout << "null"; return; }
    UNICODE_STRING s{};
    s.Buffer = const_cast<PWSTR>(name);
    s.Length = static_cast<USHORT>(std::wcslen(name) * sizeof(wchar_t));
    s.MaximumLength = s.Length;
    ULONG type = 0, data = 0, returned = 0;
    LONG status = query(&s, &type, &data, sizeof(data), &returned);
    char hex[16]; std::snprintf(hex, sizeof(hex), "0x%08lX", static_cast<unsigned long>(status));
    std::cout << "{\"status\":\"" << hex << "\",\"type\":" << type;
    if (status >= 0 && type == REG_DWORD && returned == 4) std::cout << ",\"value\":" << data;
    std::cout << "}";
}

int main() {
    try {
        const auto before = ReadPolicy();
        Validate(before);
        auto p = ProductPolicyParser::FromBinary(before);
        const std::wstring base = L"CodeIntegrity-AllowConfigurablePolicy";
        const std::wstring cks = base + L"-CustomKernelSigners";
        std::cout << "{\"readOnly\":true,\"originalHelperExecuted\":false,\"driverLoaded\":false,";
        std::cout << "\"hostRecordCount\":" << p.NumberOfPolicies() << ",\"policies\":[";
        bool first = true;
        for (const auto& name : {base, cks}) {
            if (!first) std::cout << ',';
            first = false;
            std::cout << "{\"name\":\"" << std::string(name.begin(), name.end()) << "\",\"exactPresent\":";
            bool present = false; uint32_t data = 0;
            for (size_t i = 0; i < p.NumberOfPolicies(); ++i) {
                if (p[i].GetName() == name) {
                    present = true;
                    data = p[i].GetData<PolicyValue::TypeOfUInt32>();
                }
            }
            std::cout << (present ? "true" : "false");
            if (present) std::cout << ",\"value\":" << data;
            std::cout << ",\"upstreamMutableLookup\":\"" << Lookup(p, name);
            std::cout << "\",\"upstreamConstLookup\":\"" << ConstLookup(p, name) << "\",\"nativeLicenseQuery\":";
            LicenseQuery(name.c_str()); std::cout << '}';
        }
        auto f = Fixture(); Validate(f);
        auto synthetic = ProductPolicyParser::FromBinary(f);
        std::cout << "],\"syntheticExistingNames\":[";
        first = true;
        for (const std::wstring name : {L"A", L"B", L"C"}) {
            if (!first) std::cout << ',';
            first = false;
            std::cout << "{\"name\":\"" << std::string(name.begin(), name.end()) << "\",\"lookup\":\"" << Lookup(synthetic, name) << "\"}";
        }
        bool unchanged = before == ReadPolicy();
        std::cout << "],\"registryBlobUnchangedDuringStudy\":" << (unchanged ? "true" : "false") << "}\n";
        return unchanged ? 0 : 2;
    } catch (const std::exception& e) {
        std::cerr << "Study failed: " << e.what() << '\n'; return 1;
    }
}
