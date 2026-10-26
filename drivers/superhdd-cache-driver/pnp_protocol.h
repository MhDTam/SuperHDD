#pragma once
#include "protocol.h"
/* Separate endpoint/ABI: the shipping desktop v2 cannot configure this lab. */
#if defined(SHDD_RAW_STORE) && SHDD_RAW_STORE
#define SHDD_PNP_PROTOCOL_VERSION 4u
#else
#define SHDD_PNP_PROTOCOL_VERSION 3u
#endif
#define SHDD_PNP_STATUS_IOCTL CTL_CODE(FILE_DEVICE_UNKNOWN,0x820,METHOD_BUFFERED,FILE_READ_DATA)
#define SHDD_PNP_CONFIGURE_IOCTL CTL_CODE(FILE_DEVICE_UNKNOWN,0x821,METHOD_BUFFERED,FILE_READ_DATA|FILE_WRITE_DATA)
#define SHDD_PNP_DISABLE_IOCTL CTL_CODE(FILE_DEVICE_UNKNOWN,0x822,METHOD_BUFFERED,FILE_READ_DATA|FILE_WRITE_DATA)
#define SHDD_PNP_PAUSE_IOCTL CTL_CODE(FILE_DEVICE_UNKNOWN,0x823,METHOD_BUFFERED,FILE_READ_DATA|FILE_WRITE_DATA)
#define SHDD_PNP_CLEAR_IOCTL CTL_CODE(FILE_DEVICE_UNKNOWN,0x824,METHOD_BUFFERED,FILE_READ_DATA|FILE_WRITE_DATA)
#define SHDD_RAW_DIAGNOSTIC_IOCTL CTL_CODE(FILE_DEVICE_UNKNOWN,0x825,METHOD_BUFFERED,FILE_READ_DATA)
typedef struct _SHDD_CONFIG_DIAGNOSTIC {
    ULONG Stage, Status, SourceFileFlags, SourceDeviceType;
    ULONG LowerFlags, PnpState, CacheFileFlags, CacheDeviceType;
    ULONG ActiveMutations, PendingControls, LastControlMajor, LastControlCode;
} SHDD_CONFIG_DIAGNOSTIC;
typedef struct _SHDD_PNP_STATUS {
    SHDD_STATUS Cache;
    ULONG VolumeCount;
    ULONG SourceDiskNumber;
    ULONG BackingDiskNumber;
    ULONG PnpState;
    ULONGLONG CancelledReads;
    ULONGLONG ChildCancellations;
#if defined(SHDD_RAW_STORE) && SHDD_RAW_STORE
    ULONG RawBackend, StoreLocked;
    ULONGLONG AllocatedBytes, ImportedRecords, RevalidatedRecords, ValidationReadBytes, PagingHits;
#endif
} SHDD_PNP_STATUS;
