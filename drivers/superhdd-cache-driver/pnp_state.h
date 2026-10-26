#pragma once
/* Source-shared state predicates. The host tests do not emulate Windows PnP. */
#define SHDD_PNP_STARTED 0x01
#define SHDD_PNP_STOPPED 0x02
#define SHDD_PNP_SURPRISE 0x04
#define SHDD_PNP_REMOVED 0x08
#define SHDD_PNP_SPECIAL_FILE 0x10
#define SHDD_PNP_POWER_CHANGED 0x20
#define SHDD_PNP_BLOCKED (SHDD_PNP_STOPPED | SHDD_PNP_SURPRISE | SHDD_PNP_REMOVED | SHDD_PNP_SPECIAL_FILE | SHDD_PNP_POWER_CHANGED)
static __forceinline BOOLEAN ShddPnpConfigurable(LONG state) {
    return (state & SHDD_PNP_STARTED) && !(state & SHDD_PNP_BLOCKED);
}
static __forceinline BOOLEAN ShddPnpReadable(LONG state, LONG enabled, LONG paused, SHDD_EPOCH *epoch) {
    return ShddPnpConfigurable(state) && enabled && !paused && !ShddAtomicRead(&epoch->Disabled);
}
static __forceinline BOOLEAN ShddWirePathValid(const WCHAR *path, ULONG capacity, BOOLEAN backing) {
    ULONG index, length = 0;
    const WCHAR *prefix = backing ? L"\\??\\" : L"\\Device\\";
    if (!capacity || path[capacity - 1]) return FALSE;
    while (length < capacity && path[length]) length++;
    for (index = 0; prefix[index]; index++) if (index >= length || path[index] != prefix[index]) return FALSE;
    if (length <= index || path[length - 1] == L'\\') return FALSE;
    for (index = 0; index < length; index++) {
        if (path[index] < L' ' || path[index] == L'/' || (path[index] == L':' && (!backing || index != 5))) return FALSE;
        if (path[index] == L'.' && index + 1 < length && path[index + 1] == L'.') return FALSE;
    }
    return TRUE;
}
