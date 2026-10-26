#pragma once
#include "../pnp_protocol.h"
#undef SHDD_PNP_PROTOCOL_VERSION
#define SHDD_PNP_PROTOCOL_VERSION 8u
typedef struct _SHDD_DIRECT_ATTACH {
    ULONG Version, DisposableVolumeAcknowledged;
    ULONGLONG CapacityBytes;
    WCHAR SourceDevice[260], BackingFile[520], CacheParentDevice[260];
    ULONG SourceDisk, CacheParentDisk;
    ULONGLONG SourceOffset, SourceBytes;
} SHDD_DIRECT_ATTACH;
#define SHDD_ATTACH SHDD_DIRECT_ATTACH

typedef struct _SHDD_DIRECT_STATUS {
    SHDD_STATUS Cache;
    ULONG VolumeCount, SourceDiskNumber, BackingDiskNumber, PnpState;
    ULONGLONG CancelledReads, ChildCancellations;
    ULONG RawBackend, StoreLocked;
    ULONGLONG AllocatedBytes, ImportedRecords, RevalidatedRecords, ValidationReadBytes, PagingHits;
    ULONG DirectVmBuild, PolicyVersion;
    ULONG PagingFiles, HibernationFiles, DumpFiles, PagingPassthrough;
    ULONGLONG PagingBypasses;
} SHDD_DIRECT_STATUS;
#define SHDD_PNP_STATUS SHDD_DIRECT_STATUS
C_ASSERT(sizeof(SHDD_DIRECT_ATTACH)==2120);
C_ASSERT(FIELD_OFFSET(SHDD_DIRECT_ATTACH,SourceOffset)==2104);
C_ASSERT(sizeof(SHDD_DIRECT_STATUS)==240);
