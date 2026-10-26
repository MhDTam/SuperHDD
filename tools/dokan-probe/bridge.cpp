// Native ABI adapter for a disposable, read-only, single-file host test.
// It reuses the installed Dokan runtime without installing/replacing a driver.
#include <dokan.h>
#include <cwchar>
#include <cstdint>
#include <cstdio>
#include <string>
using ReadCallback = int(__stdcall *)(void *, uint64_t, void *, DWORD, LPDWORD);
static HMODULE Library;
static DOKAN_HANDLE Instance;
static DOKAN_OPTIONS Options;
static DOKAN_OPERATIONS Operations;
static ReadCallback Read;
static void *Context;
static uint64_t Length;
static bool Initialized;
static std::wstring MountPoint;
static PVOID ExceptionTrace;
static decltype(&DokanInit) Init;
static decltype(&DokanShutdown) Shutdown;
static decltype(&DokanCreateFileSystem) Create;
static decltype(&DokanCloseHandle) Close;
static decltype(&DokanVersion) Version;
static decltype(&DokanDriverVersion) DriverVersion;
static LONG CALLBACK TraceException(EXCEPTION_POINTERS *exception) {
    if(exception->ExceptionRecord->ExceptionCode==EXCEPTION_ACCESS_VIOLATION) {
        HMODULE module=nullptr;wchar_t name[MAX_PATH]={};
        auto address=exception->ExceptionRecord->ExceptionAddress;
        GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            reinterpret_cast<LPCWSTR>(address),&module);
        if(module)GetModuleFileNameW(module,name,MAX_PATH);
        fwprintf(stderr,L"Probe access violation: module=%ls offset=0x%llx address=%p operation=%llu target=0x%llx\n",
            name,static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(address)-reinterpret_cast<uintptr_t>(module)),address,
            static_cast<unsigned long long>(exception->ExceptionRecord->ExceptionInformation[0]),
            static_cast<unsigned long long>(exception->ExceptionRecord->ExceptionInformation[1]));
        fflush(stderr);
    }
    return EXCEPTION_CONTINUE_SEARCH;
}
static int Kind(LPCWSTR name) {
    if (0 == wcscmp(name, L"\\")) return 1;
    if (0 == _wcsicmp(name, L"\\fixture.bin")) return 2;
    return 0;
}
static NTSTATUS DOKAN_CALLBACK Open(LPCWSTR name, PDOKAN_IO_SECURITY_CONTEXT,
    ACCESS_MASK access, ULONG, ULONG, ULONG disposition, ULONG createOptions, PDOKAN_FILE_INFO info) {
    const auto kind=Kind(name);
    if (!kind) return STATUS_OBJECT_NAME_NOT_FOUND;
    if ((access & (GENERIC_WRITE | GENERIC_ALL | DELETE | WRITE_DAC | WRITE_OWNER |
        FILE_WRITE_DATA | FILE_APPEND_DATA | FILE_WRITE_EA | FILE_WRITE_ATTRIBUTES)) || disposition!=FILE_OPEN)
        return STATUS_MEDIA_WRITE_PROTECTED;
    if (kind==2 && (createOptions & FILE_DIRECTORY_FILE)) return STATUS_NOT_A_DIRECTORY;
    info->IsDirectory=kind==1;
    info->Context=static_cast<ULONG64>(kind);
    return STATUS_SUCCESS;
}
static void DOKAN_CALLBACK Cleanup(LPCWSTR,PDOKAN_FILE_INFO) {}
static void DOKAN_CALLBACK FileClose(LPCWSTR,PDOKAN_FILE_INFO) {}
static NTSTATUS DOKAN_CALLBACK ReadFileData(LPCWSTR name,LPVOID buffer,DWORD count,
    LPDWORD done,LONGLONG offset,PDOKAN_FILE_INFO) {
    *done=0;
    if (Kind(name)!=2) return STATUS_FILE_IS_A_DIRECTORY;
    if (offset<0) return STATUS_INVALID_PARAMETER;
    return Read(Context,static_cast<uint64_t>(offset),buffer,count,done)==0 ? STATUS_SUCCESS : STATUS_IO_DEVICE_ERROR;
}
static NTSTATUS DOKAN_CALLBACK FileInfo(LPCWSTR name,LPBY_HANDLE_FILE_INFORMATION out,PDOKAN_FILE_INFO) {
    const auto kind=Kind(name);if(!kind)return STATUS_OBJECT_NAME_NOT_FOUND;
    ZeroMemory(out,sizeof(*out));
    out->dwFileAttributes=FILE_ATTRIBUTE_READONLY|(kind==1?FILE_ATTRIBUTE_DIRECTORY:FILE_ATTRIBUTE_ARCHIVE);
    out->nNumberOfLinks=1;out->nFileIndexLow=static_cast<DWORD>(kind);
    out->dwVolumeSerialNumber=0x53484444;
    if(kind==2){out->nFileSizeHigh=static_cast<DWORD>(Length>>32);out->nFileSizeLow=static_cast<DWORD>(Length);}
    return STATUS_SUCCESS;
}
static NTSTATUS DOKAN_CALLBACK Find(LPCWSTR name,PFillFindData fill,PDOKAN_FILE_INFO info) {
    if(Kind(name)!=1)return STATUS_NOT_A_DIRECTORY;
    WIN32_FIND_DATAW data{};data.dwFileAttributes=FILE_ATTRIBUTE_READONLY|FILE_ATTRIBUTE_ARCHIVE;
    data.nFileSizeHigh=static_cast<DWORD>(Length>>32);data.nFileSizeLow=static_cast<DWORD>(Length);
    wcscpy_s(data.cFileName,L"fixture.bin");fill(&data,info);return STATUS_SUCCESS;
}
static NTSTATUS DOKAN_CALLBACK Space(PULONGLONG free,PULONGLONG total,PULONGLONG available,PDOKAN_FILE_INFO) {
    *free=0;*available=0;*total=Length;return STATUS_SUCCESS;
}
static NTSTATUS DOKAN_CALLBACK Volume(LPWSTR label,DWORD labelCount,LPDWORD serial,LPDWORD maxPart,
    LPDWORD flags,LPWSTR fs,DWORD fsCount,PDOKAN_FILE_INFO) {
    if(wcscpy_s(label,labelCount,L"SuperHDD test") || wcscpy_s(fs,fsCount,L"SHDDProbe"))return STATUS_BUFFER_TOO_SMALL;
    *serial=0x53484444;*maxPart=255;
    *flags=FILE_READ_ONLY_VOLUME|FILE_CASE_PRESERVED_NAMES|FILE_UNICODE_ON_DISK;
    return STATUS_SUCCESS;
}
static NTSTATUS DOKAN_CALLBACK Mounted(LPCWSTR name,PDOKAN_FILE_INFO) {
    return _wcsicmp(name,MountPoint.c_str())==0 ||
        (MountPoint==L"S:\\" && _wcsicmp(name,L"S:")==0) ? STATUS_SUCCESS : STATUS_ACCESS_DENIED;
}
static NTSTATUS DOKAN_CALLBACK Unmounted(PDOKAN_FILE_INFO) {return STATUS_SUCCESS;}
static int MountAt(ReadCallback read,void *context,uint64_t length,const wchar_t *point) {
    if(Instance || Library || !read || length!=16ULL*1024*1024)return 1001;
    MountPoint=point;
    ExceptionTrace=AddVectoredExceptionHandler(1,TraceException);
    fprintf(stderr,"probe: load\n");fflush(stderr);
    Library=LoadLibraryExW(L"dokan2.dll",nullptr,LOAD_LIBRARY_SEARCH_SYSTEM32);
    if(!Library)return 1003;
    // Dokan schedules asynchronous notifications beyond instance close. Keep its
    // executable code mapped for this worker process, rather than racing DLL
    // unload against those notifications. This does not keep a mount alive.
    HMODULE pinned=nullptr;
    if(!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN|GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        reinterpret_cast<LPCWSTR>(Library),&pinned))return 1006;
#define LOAD(field,name) field=reinterpret_cast<decltype(field)>(GetProcAddress(Library,name));if(!field)return 1004
    LOAD(Init,"DokanInit");LOAD(Shutdown,"DokanShutdown");LOAD(Create,"DokanCreateFileSystem");
    LOAD(Close,"DokanCloseHandle");LOAD(Version,"DokanVersion");LOAD(DriverVersion,"DokanDriverVersion");
#undef LOAD
    Init();Initialized=true;
    fprintf(stderr,"probe: initialized\n");fflush(stderr);
    const auto dllVersion=Version(), driverVersion=DriverVersion();
    if(dllVersion!=DOKAN_VERSION || driverVersion!=DOKAN_DRIVER_VERSION){
        fprintf(stderr,"Dokan version query: DLL %lu (expected %d), driver ABI %lu (expected %d)\n",dllVersion,DOKAN_VERSION,driverVersion,DOKAN_DRIVER_VERSION);return 1005;
    }
    Read=read;Context=context;Length=length;
    Options.Version=DOKAN_VERSION;
    Options.Options=DOKAN_OPTION_WRITE_PROTECT|DOKAN_OPTION_CURRENT_SESSION;
    Options.MountPoint=MountPoint.c_str();Options.Timeout=30000;
    Options.AllocationUnitSize=4096;Options.SectorSize=512;
    Operations.ZwCreateFile=Open;Operations.Cleanup=Cleanup;Operations.CloseFile=FileClose;
    Operations.ReadFile=ReadFileData;Operations.GetFileInformation=FileInfo;Operations.FindFiles=Find;
    Operations.GetDiskFreeSpace=Space;Operations.GetVolumeInformation=Volume;
    Operations.Mounted=Mounted;Operations.Unmounted=Unmounted;
    fprintf(stderr,"probe: create\n");fflush(stderr);
    auto result=Create(&Options,&Operations,&Instance);
    fprintf(stderr,"probe: create result %d\n",result);fflush(stderr);
    return result;
}
extern "C" __declspec(dllexport) int __stdcall ShddMount(ReadCallback read,void *context,uint64_t length) {
    wchar_t existing[4096];
    if(QueryDosDeviceW(L"S:",existing,4096) || GetLastError()!=ERROR_FILE_NOT_FOUND)return 1002;
    return MountAt(read,context,length,L"S:\\");
}
extern "C" __declspec(dllexport) int __stdcall ShddMountDirectory(ReadCallback read,void *context,uint64_t length,LPCWSTR point) {
    // Only a newly created, empty directory under a GUID-named D test parent.
    const std::wstring prefix=L"D:\\SuperHDD-DokanDirectory-";
    if(!point)return 1010;
    const std::wstring path=point;
    if(path.size()!=prefix.size()+32+8 || path.compare(0,prefix.size(),prefix)!=0 ||
       path.substr(prefix.size()+32)!=L"\\visible")return 1011;
    for(size_t i=prefix.size();i<prefix.size()+32;++i)
        if(!((path[i]>=L'0' && path[i]<=L'9') || (path[i]>=L'a' && path[i]<=L'f')))return 1012;
    auto attrs=GetFileAttributesW(point);
    auto parent=GetFileAttributesW(path.substr(0,prefix.size()+32).c_str());
    if(attrs==INVALID_FILE_ATTRIBUTES || parent==INVALID_FILE_ATTRIBUTES ||
       !(attrs&FILE_ATTRIBUTE_DIRECTORY) || !(parent&FILE_ATTRIBUTE_DIRECTORY) ||
       (attrs&FILE_ATTRIBUTE_REPARSE_POINT) || (parent&FILE_ATTRIBUTE_REPARSE_POINT))return 1013;
    WIN32_FIND_DATAW entry{};
    HANDLE search=FindFirstFileW((path+L"\\*").c_str(),&entry);
    if(search==INVALID_HANDLE_VALUE)return 1014;
    bool empty=true;
    do {if(wcscmp(entry.cFileName,L".") && wcscmp(entry.cFileName,L".."))empty=false;}
    while(FindNextFileW(search,&entry));
    DWORD error=GetLastError();FindClose(search);
    if(!empty || error!=ERROR_NO_MORE_FILES)return 1015;
    return MountAt(read,context,length,point);
}
extern "C" __declspec(dllexport) int __stdcall ShddUnmount() {
    // Close only the private instance obtained above. Never unmount by letter.
    if(Instance){Close(Instance);Instance=nullptr;}
    fprintf(stderr,"probe: instance closed\n");fflush(stderr);
    if(Initialized){Shutdown();Initialized=false;}
    fprintf(stderr,"probe: shutdown\n");fflush(stderr);
    if(Library){FreeLibrary(Library);Library=nullptr;}
    if(ExceptionTrace){RemoveVectoredExceptionHandler(ExceptionTrace);ExceptionTrace=nullptr;}
    Read=nullptr;Context=nullptr;MountPoint.clear();return 0;
}
