/* Paging-capable storage-volume v8 candidate. Source stays mounted. Backing
 * paging-path enrollment and reserved worker resources are required before
 * enabling paging reads. Never install an unqualified image on user disks. */
#include <ntifs.h>
#include <ntddstor.h>
#include <ntdddisk.h>
#include <ntddvol.h>
#include <mountdev.h>
#include <wdmsec.h>
#include <bcrypt.h>
#include "protocol_v8.h"
#include "source_policy.h"
#include "usage_state.h"
#ifndef SHDD_DIRECT_VM_BUILD
#define SHDD_DIRECT_VM_BUILD 0
#endif
#include "../cache_state.h"
#include "../cache_invalidation.h"
#include "../pnp_state.h"
#ifndef SHDD_RAW_STORE
#define SHDD_RAW_STORE 0
#endif
#if SHDD_RAW_STORE
#include "../raw_store.h"
#endif
#ifndef SHDD_ENABLE_EXPERIMENTAL_ATTACH
#define SHDD_ENABLE_EXPERIMENTAL_ATTACH 0
#endif
#define TAG 'pHSS'
typedef struct _SLOT {
    ULONGLONG Block, Generation, Checksum;
    volatile LONG64 InvalidatedAt;
#if SHDD_RAW_STORE
    UCHAR Digest[32]; BOOLEAN RecordValid;
#endif
} SLOT;
typedef struct _VOLUME {
    BOOLEAN Control;
    LIST_ENTRY DeviceLink;
    PDEVICE_OBJECT Self, Lower, Pdo;
    IO_REMOVE_LOCK RemoveLock;
    volatile LONG SpecialUsage;
    volatile LONG PagingFiles, HibernationFiles, DumpFiles;
    volatile LONG64 PagingBypasses;
    KEVENT UsageGate;
    volatile LONG PnpState, Enabled, Paused, PendingReads, ThreadStopping, BackingIo;
    BOOLEAN SessionLive;
    IO_CSQ Csq;
    KSPIN_LOCK QueueLock;
    LIST_ENTRY Queue;
    KEVENT QueueReady;
    EX_RUNDOWN_REF Session;
    HANDLE Thread, Backing;
    PFILE_OBJECT BackingFile;
    /* Single reader owns these resources until its thread has terminated. */
    PVOID ScratchAllocation;
    UCHAR *Scratch;
    PIRP TransferIrp;
    PMDL TransferMdl;
    BCRYPT_ALG_HANDLE DigestAlgorithm;
    BCRYPT_HASH_HANDLE DigestHash;
    PUCHAR DigestObject;
#if SHDD_RAW_STORE
    PDEVICE_OBJECT RawDevice;
    BOOLEAN StoreLocked;
    BOOLEAN BackingPagingPath;
    SHDD_STORE_HEADER StoreHeader;
    PVOID MetadataAllocation;
    UCHAR *Metadata;
    volatile LONG64 ImportedRecords, RevalidatedRecords, ValidationReadBytes, PagingHits;
#endif
    SHDD_EPOCH Epoch;
    volatile LONG64 GlobalInvalidatedAt;
    SHDD_DIRTY_RANGE DirtyRanges[SHDD_DIRTY_RANGE_LIMIT];
    ULONG DirtyRangeCount;
    volatile LONG PendingControls, LastControlMajor, LastControlCode;
    SLOT *Slots;
    ULONG SlotCount, SourceDisk, BackingDisk;
    ULONGLONG Capacity, SourceLength;
    volatile LONG64 Hits, Misses, SourceReadBytes, SsdReadBytes, FillBytes;
    volatile LONG64 Evictions, Errors, Bypasses, CancelledReads, ChildCancellations;
} VOLUME;
#include "digest.inc"
typedef struct _CHILD { KEVENT Done; IO_STATUS_BLOCK IoStatus; } CHILD;
typedef struct _USAGE_CHANGE {
    VOLUME *Volume;
    SHDD_USAGE_COUNTS Before;
} USAGE_CHANGE;
static PDEVICE_OBJECT gControl;
static LIST_ENTRY gVolumes;
static KMUTEX gConfiguration;
static VOLUME *gActive;
static ULONG gVolumeCount;
#if SHDD_RAW_STORE
static SHDD_CONFIG_DIAGNOSTIC gDiagnostic;
#define ConfigStep(n) (gDiagnostic.Stage=(n))
#else
#define ConfigStep(n) ((void)0)
#endif
#if SHDD_RAW_STORE
static UNICODE_STRING gLink = RTL_CONSTANT_STRING(L"\\DosDevices\\SuperHDDCacheV8");
#else
static UNICODE_STRING gLink = RTL_CONSTANT_STRING(L"\\DosDevices\\SuperHDDCachePnpLab");
#endif
static const GUID gClass = {0xe3b5a745,0x87af,0x44a2,{0x91,0x90,0x0d,0x66,0xe7,0x4b,0x82,0x53}};
DRIVER_INITIALIZE DriverEntry;
static DRIVER_ADD_DEVICE AddDevice;
static DRIVER_DISPATCH Dispatch;
static DRIVER_UNLOAD Unload;
static IO_COMPLETION_ROUTINE ForwardDone, MutationDone, ControlMutationDone, ChildDone, SignalDone, UsageDone, UsageQueryDone, CancelPnpDone, PowerReadyDone;
static IO_CSQ_INSERT_IRP_EX QueueInsert;
static IO_CSQ_REMOVE_IRP QueueRemove;
static IO_CSQ_PEEK_NEXT_IRP QueuePeek;
static IO_CSQ_ACQUIRE_LOCK QueueAcquire;
static IO_CSQ_RELEASE_LOCK QueueRelease;
static IO_CSQ_COMPLETE_CANCELED_IRP QueueCancel;
static KSTART_ROUTINE Reader;
#if SHDD_RAW_STORE
static NTSTATUS RawIo(VOLUME *volume,PIRP original,UCHAR major,PVOID buffer,ULONG bytes,ULONGLONG offset);
static NTSTATUS RawCommit(VOLUME *volume,PIRP original,ULONG slot,const UCHAR digest[32],ULONGLONG block);
static VOID RawClose(VOLUME *volume);
static NTSTATUS RawClear(VOLUME *volume);
#endif

static NTSTATUS Complete(PIRP irp, NTSTATUS status, ULONG_PTR bytes) {
    irp->IoStatus.Status = status; irp->IoStatus.Information = bytes;
    IoCompleteRequest(irp, IO_NO_INCREMENT); return status;
}
static ULONGLONG Hash(const UCHAR *bytes, ULONG count) {
    ULONGLONG hash = 0xcbf29ce484222325ULL; ULONG i;
    for (i = 0; i < count; i++) { hash ^= bytes[i]; hash *= 0x100000001b3ULL; }
    return hash;
}
static BOOLEAN Mutation(PIO_STACK_LOCATION stack,PIRP irp) {
    UCHAR major=stack->MajorFunction;
    /* These documented queries cannot change source bytes. Flushes retain
     * trust: every underlying write is already invalidated before forwarding. */
    if(major==IRP_MJ_DEVICE_CONTROL){
        ULONG code=stack->Parameters.DeviceIoControl.IoControlCode;
        if(code==IOCTL_STORAGE_QUERY_PROPERTY || code==IOCTL_DISK_GET_LENGTH_INFO
            || code==IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS || code==IOCTL_STORAGE_GET_DEVICE_NUMBER
            || code==IOCTL_VOLUME_IS_CLUSTERED || code==IOCTL_MOUNTDEV_QUERY_DEVICE_NAME
            || code==IOCTL_MOUNTDEV_QUERY_UNIQUE_ID || code==IOCTL_MOUNTDEV_QUERY_STABLE_GUID
            || code==IOCTL_MOUNTDEV_QUERY_SUGGESTED_LINK_NAME)
            return FALSE;
        if(code==IOCTL_STORAGE_MANAGE_DATA_SET_ATTRIBUTES
            && ShddDsmReadOnly(irp->AssociatedIrp.SystemBuffer,stack->Parameters.DeviceIoControl.InputBufferLength))return FALSE;
    }
    return major == IRP_MJ_WRITE || major == IRP_MJ_DEVICE_CONTROL
        || major == IRP_MJ_INTERNAL_DEVICE_CONTROL || major == IRP_MJ_FILE_SYSTEM_CONTROL;
}
static VOID InvalidateSource(VOLUME *volume, PIO_STACK_LOCATION stack,PIRP irp) {
    LONG64 stamp=ShddGeneration(&volume->Epoch); KIRQL irql;
    if(stack->MajorFunction!=IRP_MJ_WRITE){
        SHDD_DIRTY_RANGE ranges[SHDD_DSM_RANGE_LIMIT];ULONG count=0,i;
        KeAcquireSpinLock(&volume->QueueLock,&irql);
        if(ShddAtomicRead(&volume->Enabled) && stack->MajorFunction==IRP_MJ_DEVICE_CONTROL
            && stack->Parameters.DeviceIoControl.IoControlCode==IOCTL_STORAGE_MANAGE_DATA_SET_ATTRIBUTES
            && ShddTrimRanges(irp->AssociatedIrp.SystemBuffer,stack->Parameters.DeviceIoControl.InputBufferLength,
                volume->SourceLength,ranges,&count)){
            for(i=0;i<count;i++)if(!ShddRememberRange(volume->DirtyRanges,&volume->DirtyRangeCount,&ranges[i],stamp))
                ShddAdvanceFloor(&volume->GlobalInvalidatedAt,stamp);
        }else ShddAdvanceFloor(&volume->GlobalInvalidatedAt,stamp);
        KeReleaseSpinLock(&volume->QueueLock,irql);return;
    }
    KeAcquireSpinLock(&volume->QueueLock,&irql);
    if(ShddAtomicRead(&volume->Enabled)&&volume->Slots&&volume->SlotCount){
        LONGLONG offset=stack->Parameters.Write.ByteOffset.QuadPart;
        ULONG length=stack->Parameters.Write.Length;
        if(offset<0 || (ULONGLONG)offset>volume->SourceLength || length>volume->SourceLength-(ULONGLONG)offset){
            ShddAdvanceFloor(&volume->GlobalInvalidatedAt,stamp);
        }else if(length){
            ULONGLONG first=(ULONGLONG)offset/SHDD_BLOCK_BYTES;
            ULONGLONG last=((ULONGLONG)offset+length-1)/SHDD_BLOCK_BYTES,block;
            /* Bound time spent holding the spinlock for large writes. */
            if(last-first>=128)ShddAdvanceFloor(&volume->GlobalInvalidatedAt,stamp);
            else for(block=first;block<=last;block++)
                ShddAdvanceFloor(&volume->Slots[block%volume->SlotCount].InvalidatedAt,stamp);
        }
    }
    KeReleaseSpinLock(&volume->QueueLock,irql);
}
static BOOLEAN SlotTrusted(VOLUME *volume,SLOT *slot,ULONGLONG block){
    KIRQL irql;BOOLEAN trusted;
    KeAcquireSpinLock(&volume->QueueLock,&irql);
    trusted=ShddTrustValid(slot->Generation,InterlockedCompareExchange64(&slot->InvalidatedAt,0,0),
        InterlockedCompareExchange64(&volume->GlobalInvalidatedAt,0,0))
        && ShddRangesTrust(volume->DirtyRanges,volume->DirtyRangeCount,block,slot->Generation);
    KeReleaseSpinLock(&volume->QueueLock,irql);return trusted;
}
_Use_decl_annotations_
static NTSTATUS ForwardDone(PDEVICE_OBJECT device, PIRP irp, PVOID context) {
    VOLUME *volume = context; UNREFERENCED_PARAMETER(device);
    if (irp->PendingReturned) IoMarkIrpPending(irp);
    if (volume) IoReleaseRemoveLock(&volume->RemoveLock, irp);
    return STATUS_CONTINUE_COMPLETION;
}
_Use_decl_annotations_
static NTSTATUS MutationDone(PDEVICE_OBJECT device, PIRP irp, PVOID context) {
    VOLUME *volume = context;
    if (volume) ShddMutationEnd(&volume->Epoch);
    return ForwardDone(device, irp, context);
}
_Use_decl_annotations_
static NTSTATUS ControlMutationDone(PDEVICE_OBJECT device, PIRP irp, PVOID context) {
    VOLUME *volume = context;
    if(volume)InterlockedDecrement(&volume->PendingControls);
    return MutationDone(device, irp, context);
}
static NTSTATUS Forward(VOLUME *volume, PIRP irp) {
    PIO_STACK_LOCATION stack=IoGetCurrentIrpStackLocation(irp);
    BOOLEAN mutation = Mutation(stack,irp);
    BOOLEAN control=mutation && stack->MajorFunction!=IRP_MJ_WRITE && stack->MajorFunction!=IRP_MJ_FLUSH_BUFFERS;
    if (mutation) {ShddMutationBegin(&volume->Epoch);InvalidateSource(volume,stack,irp);}
    if(control){
        InterlockedIncrement(&volume->PendingControls);
        InterlockedExchange(&volume->LastControlMajor,stack->MajorFunction);
        InterlockedExchange(&volume->LastControlCode,(LONG)stack->Parameters.DeviceIoControl.IoControlCode);
#if SHDD_RAW_STORE
        InterlockedExchange((volatile LONG*)&gDiagnostic.LastControlMajor,stack->MajorFunction);
        InterlockedExchange((volatile LONG*)&gDiagnostic.LastControlCode,(LONG)stack->Parameters.DeviceIoControl.IoControlCode);
#endif
    }
    IoCopyCurrentIrpStackLocationToNext(irp);
    IoSetCompletionRoutine(irp, control ? ControlMutationDone : (mutation ? MutationDone : ForwardDone), volume, TRUE, TRUE, TRUE);
    return IoCallDriver(volume->Lower, irp);
}
static VOID ReleaseRead(VOLUME *volume, PIRP irp, BOOLEAN forward, NTSTATUS status, ULONG_PTR bytes) {
    InterlockedDecrement(&volume->PendingReads);
    ExReleaseRundownProtection(&volume->Session);
    if (forward) {
        InterlockedIncrement64(&volume->Bypasses);
        if(irp->Flags&(IRP_PAGING_IO|IRP_SYNCHRONOUS_PAGING_IO))InterlockedIncrement64(&volume->PagingBypasses);
        Forward(volume, irp);
    }
    else { IoReleaseRemoveLock(&volume->RemoveLock, irp); Complete(irp, status, bytes); }
}
_Use_decl_annotations_
static VOID QueueAcquire(PIO_CSQ csq, PKIRQL irql) {
    VOLUME *volume = CONTAINING_RECORD(csq, VOLUME, Csq);
    KeAcquireSpinLock(&volume->QueueLock, irql);
}
_Use_decl_annotations_
static VOID QueueRelease(PIO_CSQ csq, KIRQL irql) {
    VOLUME *volume = CONTAINING_RECORD(csq, VOLUME, Csq);
    KeReleaseSpinLock(&volume->QueueLock, irql);
}
_Use_decl_annotations_
static NTSTATUS QueueInsert(PIO_CSQ csq, PIRP irp, PVOID context) {
    VOLUME *volume = CONTAINING_RECORD(csq, VOLUME, Csq); UNREFERENCED_PARAMETER(context);
    if (!ShddPnpReadable(ShddAtomicRead(&volume->PnpState), ShddAtomicRead(&volume->Enabled),
            ShddAtomicRead(&volume->Paused), &volume->Epoch) || ShddAtomicRead(&volume->BackingIo)) return STATUS_DEVICE_NOT_READY;
    InsertTailList(&volume->Queue, &irp->Tail.Overlay.ListEntry);
    KeSetEvent(&volume->QueueReady, IO_NO_INCREMENT, FALSE); return STATUS_SUCCESS;
}
_Use_decl_annotations_
static VOID QueueRemove(PIO_CSQ csq, PIRP irp) {
    VOLUME *volume = CONTAINING_RECORD(csq, VOLUME, Csq);
    RemoveEntryList(&irp->Tail.Overlay.ListEntry);
    if (IsListEmpty(&volume->Queue)) KeClearEvent(&volume->QueueReady);
}
_Use_decl_annotations_
static PIRP QueuePeek(PIO_CSQ csq, PIRP irp, PVOID context) {
    VOLUME *volume = CONTAINING_RECORD(csq, VOLUME, Csq);
    PLIST_ENTRY next = irp ? irp->Tail.Overlay.ListEntry.Flink : volume->Queue.Flink;
    UNREFERENCED_PARAMETER(context);
    return next == &volume->Queue ? NULL : CONTAINING_RECORD(next, IRP, Tail.Overlay.ListEntry);
}
_Use_decl_annotations_
static VOID QueueCancel(PIO_CSQ csq, PIRP irp) {
    VOLUME *volume = CONTAINING_RECORD(csq, VOLUME, Csq);
    InterlockedIncrement64(&volume->CancelledReads);
    ReleaseRead(volume, irp, FALSE, STATUS_CANCELLED, 0);
}
_Use_decl_annotations_
static NTSTATUS ChildDone(PDEVICE_OBJECT device, PIRP irp, PVOID context) {
    CHILD *child = context; UNREFERENCED_PARAMETER(device);
    if (child) { child->IoStatus = irp->IoStatus; KeSetEvent(&child->Done, IO_NO_INCREMENT, FALSE); }
    return STATUS_MORE_PROCESSING_REQUIRED;
}
/* Own the child until completion, including after cancellation. A misbehaving
 * lower driver which never completes a cancelled request can still stall teardown.
 * The dedicated worker remains alive and retains the MDL/buffer in that case. */
static NTSTATUS Transfer(VOLUME *volume, PIRP original, PDEVICE_OBJECT target, PFILE_OBJECT file,
        UCHAR major, PVOID bytes, ULONG length, ULONGLONG offset, PULONG_PTR transferred) {
    CHILD child; PIRP request; PIO_STACK_LOCATION stack; LARGE_INTEGER position, interval;
    BOOLEAN cancelSent = FALSE, reserved = original != NULL; ULONG polls = 0;
    *transferred = 0; RtlZeroMemory(&child, sizeof(child));
    KeInitializeEvent(&child.Done, NotificationEvent, FALSE); position.QuadPart = (LONGLONG)offset;
    /* Explicit nonpaged MDL ownership avoids buffered-I/O completion copying and
     * the extra pool-page locks taken by IoBuildAsynchronousFsdRequest. */
    if (target->Flags & DO_BUFFERED_IO) return STATUS_NOT_SUPPORTED;
    if (reserved) {
        if (!volume->TransferIrp || !volume->TransferMdl || length > SHDD_BLOCK_BYTES)
            return STATUS_INSUFFICIENT_RESOURCES;
        request = volume->TransferIrp;
        if (target->StackSize > request->StackCount) return STATUS_NOT_SUPPORTED;
        IoReuseIrp(request, STATUS_NOT_SUPPORTED);
        MmInitializeMdl(volume->TransferMdl, bytes, length);
        request->MdlAddress = volume->TransferMdl;
    } else request = IoAllocateIrp(target->StackSize, FALSE);
    if (!request) return STATUS_INSUFFICIENT_RESOURCES;
    if (!reserved && !IoAllocateMdl(bytes, length, FALSE, FALSE, request)) { IoFreeIrp(request); return STATUS_INSUFFICIENT_RESOURCES; }
    MmBuildMdlForNonPagedPool(request->MdlAddress);
    request->UserBuffer = bytes; request->Tail.Overlay.Thread = PsGetCurrentThread();
    request->Tail.Overlay.OriginalFileObject = file; request->RequestorMode = KernelMode;
    request->Flags = IRP_NOCACHE | (major == IRP_MJ_READ ? IRP_READ_OPERATION : IRP_WRITE_OPERATION);
    if (original) request->Flags |= original->Flags & (IRP_PAGING_IO | IRP_SYNCHRONOUS_PAGING_IO);
    stack = IoGetNextIrpStackLocation(request); stack->MajorFunction = major; stack->FileObject = file;
    stack->Parameters.Read.ByteOffset = position; stack->Parameters.Read.Length = length;
    IoSetCompletionRoutine(request, ChildDone, &child, TRUE, TRUE, TRUE);
    /* Any source read arriving while backing I/O is outstanding bypasses the
     * cache, even when generated on another filesystem worker thread. This is
     * an additional recursion guard, not proof of independent VHD host media. */
    {
        BOOLEAN backing = file != NULL;
#if SHDD_RAW_STORE
        backing = backing || target == volume->RawDevice;
#endif
        if (backing) InterlockedIncrement(&volume->BackingIo);
    IoCallDriver(target, request); interval.QuadPart = -1000000LL;
    while (KeWaitForSingleObject(&child.Done, Executive, KernelMode, FALSE, &interval) == STATUS_TIMEOUT) {
        polls++;
        if (!cancelSent && ((original && (original->Cancel || !ShddAtomicRead(&volume->Enabled)
                || ShddAtomicRead(&volume->Epoch.Disabled))) || polls >= 100)) {
            cancelSent = TRUE; InterlockedIncrement64(&volume->ChildCancellations); IoCancelIrp(request);
        }
    }
    *transferred = NT_SUCCESS(child.IoStatus.Status) ? child.IoStatus.Information : 0;
    if (!reserved && request->MdlAddress) {
        IoFreeMdl(request->MdlAddress); request->MdlAddress = NULL;
    }
    request->MdlAddress = NULL;
    if (!reserved) IoFreeIrp(request);
        if (backing) InterlockedDecrement(&volume->BackingIo);
    }
    return child.IoStatus.Status;
}
static NTSTATUS CacheIo(VOLUME *volume,PIRP original,UCHAR major,PVOID buffer,ULONG bytes,ULONGLONG offset,PULONG_PTR returned){
#if SHDD_RAW_STORE
    NTSTATUS status=RawIo(volume,original,major,buffer,bytes,volume->StoreHeader.DataOffset+offset);
    *returned=NT_SUCCESS(status)?bytes:0;return status;
#else
    return Transfer(volume,original,IoGetRelatedDeviceObject(volume->BackingFile),volume->BackingFile,major,buffer,bytes,offset,returned);
#endif
}
static VOID Read(VOLUME *volume, PIRP irp) {
    PIO_STACK_LOCATION stack = IoGetCurrentIrpStackLocation(irp);
    ULONG length = stack->Parameters.Read.Length, done = 0;
    ULONGLONG start = (ULONGLONG)stack->Parameters.Read.ByteOffset.QuadPart;
    UCHAR *output = MmGetSystemAddressForMdlSafe(irp->MdlAddress, NormalPagePriority | MdlMappingNoExecute);
    UCHAR *scratch = volume->Scratch; NTSTATUS status = STATUS_SUCCESS; BOOLEAN bypass = FALSE;
    if (!output || !scratch) {
        ReleaseRead(volume, irp, TRUE, STATUS_SUCCESS, 0); return;
    }
    while (done < length) {
        ULONGLONG position = start + done, block = position / SHDD_BLOCK_BYTES;
        ULONG blockOffset = (ULONG)(position % SHDD_BLOCK_BYTES), count = ShddSliceBytes(position, length - done);
        SLOT *slot = &volume->Slots[block % volume->SlotCount]; ULONG_PTR returned = 0;
        LONG64 epoch = ShddCaptureEpoch(&volume->Epoch); BOOLEAN hit = FALSE;
        if (irp->Cancel) { status = STATUS_CANCELLED; break; }
        if (!ShddPnpReadable(ShddAtomicRead(&volume->PnpState), ShddAtomicRead(&volume->Enabled),
                ShddAtomicRead(&volume->Paused), &volume->Epoch)) { bypass = TRUE; break; }
        if (epoch && SlotTrusted(volume,slot,block) && slot->Block == block) {
            status = CacheIo(volume, irp,
                IRP_MJ_READ, scratch, SHDD_BLOCK_BYTES, (ULONGLONG)(slot - volume->Slots) * SHDD_BLOCK_BYTES, &returned);
            if (NT_SUCCESS(status) && returned == SHDD_BLOCK_BYTES && Hash(scratch, SHDD_BLOCK_BYTES) == slot->Checksum) {
                /* A concurrent source mutation is an expected cache miss,
                 * not corruption or a backing-device error. */
                hit = ShddEpochValid(&volume->Epoch, epoch);
#if SHDD_RAW_STORE
                { UCHAR digest[32];status=Digest(volume,scratch,SHDD_BLOCK_BYTES,digest);if(!NT_SUCCESS(status)||!ShddDigestEqual(digest,slot->Digest)){hit=FALSE;slot->Generation=0;slot->RecordValid=FALSE;InterlockedIncrement64(&volume->Errors);if(!NT_SUCCESS(status))ShddDisable(&volume->Epoch);} }
                if(hit&&(irp->Flags&IRP_PAGING_IO))InterlockedIncrement64(&volume->PagingHits);
#endif
                if(hit){InterlockedIncrement64(&volume->Hits);InterlockedAdd64(&volume->SsdReadBytes,returned);}
            } else { slot->Generation = 0;
#if SHDD_RAW_STORE
                slot->RecordValid=FALSE;
#endif
                InterlockedIncrement64(&volume->Errors); }
        }
        if (!hit) {
            InterlockedIncrement64(&volume->Misses);
            status = Transfer(volume, irp, volume->Lower, NULL, IRP_MJ_READ, scratch, SHDD_BLOCK_BYTES,
                block * SHDD_BLOCK_BYTES, &returned);
            InterlockedAdd64(&volume->SourceReadBytes, returned);
            if (!NT_SUCCESS(status) || returned != SHDD_BLOCK_BYTES) { bypass = TRUE; break; }
            if (ShddEpochValid(&volume->Epoch, epoch) && !irp->Cancel) {
#if SHDD_RAW_STORE
                UCHAR digest[32];BOOLEAN reused=FALSE;
                status=Digest(volume,scratch,SHDD_BLOCK_BYTES,digest);
                if(!NT_SUCCESS(status)){InterlockedIncrement64(&volume->Errors);ShddDisable(&volume->Epoch);bypass=TRUE;break;}
                if(slot->RecordValid&&slot->Block==block){
                    InterlockedAdd64(&volume->ValidationReadBytes,SHDD_BLOCK_BYTES);
                    if(ShddDigestEqual(digest,slot->Digest)){
                        slot->Checksum=Hash(scratch,SHDD_BLOCK_BYTES);slot->Generation=(ULONGLONG)epoch;
                        InterlockedIncrement64(&volume->RevalidatedRecords);reused=TRUE;
                    }
                }
                if(!reused){
#endif
                if (slot->Generation == (ULONGLONG)epoch && slot->Block != block) InterlockedIncrement64(&volume->Evictions);
                slot->Generation = 0;
#if SHDD_RAW_STORE
                slot->RecordValid=FALSE;
#endif
                status = CacheIo(volume, irp,
                    IRP_MJ_WRITE, scratch, SHDD_BLOCK_BYTES, (ULONGLONG)(slot - volume->Slots) * SHDD_BLOCK_BYTES, &returned);
                if (NT_SUCCESS(status) && returned == SHDD_BLOCK_BYTES && ShddEpochValid(&volume->Epoch, epoch)) {
#if SHDD_RAW_STORE
                    slot->RecordValid=FALSE;
                    status=RawCommit(volume,irp,(ULONG)(slot-volume->Slots),digest,block);
                    if(NT_SUCCESS(status)&&ShddEpochValid(&volume->Epoch,epoch)){
                        RtlCopyMemory(slot->Digest,digest,32);slot->RecordValid=TRUE;
#endif
                    slot->Block = block; slot->Checksum = Hash(scratch, SHDD_BLOCK_BYTES); slot->Generation = (ULONGLONG)epoch;
                    InterlockedAdd64(&volume->FillBytes, returned);
#if SHDD_RAW_STORE
                    }else if(!NT_SUCCESS(status)) InterlockedIncrement64(&volume->Errors);
#endif
                } else if(!NT_SUCCESS(status)||returned!=SHDD_BLOCK_BYTES) InterlockedIncrement64(&volume->Errors);
#if SHDD_RAW_STORE
                }
#endif
            }
        }
        if (irp->Cancel) { status = STATUS_CANCELLED; break; }
        RtlCopyMemory(output + done, scratch + blockOffset, count); done += count; status = STATUS_SUCCESS;
    }
    if (irp->Cancel) { status = STATUS_CANCELLED; bypass = FALSE; InterlockedIncrement64(&volume->CancelledReads); }
    ReleaseRead(volume, irp, bypass, status, done);
}
_Use_decl_annotations_
static VOID Reader(PVOID context) {
    VOLUME *volume = context;
    for (;;) {
        PIRP irp = IoCsqRemoveNextIrp(&volume->Csq, NULL);
        if (irp) { Read(volume, irp); continue; }
        if (ShddAtomicRead(&volume->ThreadStopping)) break;
        KeWaitForSingleObject(&volume->QueueReady, Executive, KernelMode, FALSE, NULL);
    }
    PsTerminateSystemThread(STATUS_SUCCESS);
}
static VOID DeleteBacking(VOLUME *volume) {
#if SHDD_RAW_STORE
    RawClose(volume);
#else
    FILE_DISPOSITION_INFORMATION disposition; IO_STATUS_BLOCK iosb;
    if (volume->BackingFile) { ObDereferenceObject(volume->BackingFile); volume->BackingFile = NULL; }
    if (volume->Backing) {
        disposition.DeleteFile = TRUE;
        NTSTATUS status = ZwSetInformationFile(volume->Backing, &iosb, &disposition, sizeof(disposition), FileDispositionInformation);
        if (status == STATUS_PENDING) ZwWaitForSingleObject(volume->Backing, FALSE, NULL);
        ZwClose(volume->Backing); volume->Backing = NULL;
    }
#endif
}
/* gConfiguration held; callers retain a remove lock or are in REMOVE_DEVICE. */
static VOID DisableSession(VOLUME *volume) {
    KIRQL irql; PIRP irp;
    KeAcquireSpinLock(&volume->QueueLock, &irql); InterlockedExchange(&volume->Enabled, 0);
    ShddDisable(&volume->Epoch); KeReleaseSpinLock(&volume->QueueLock, irql);
    while ((irp = IoCsqRemoveNextIrp(&volume->Csq, NULL)) != NULL) ReleaseRead(volume, irp, TRUE, STATUS_SUCCESS, 0);
    if (volume->SessionLive) { ExWaitForRundownProtectionRelease(&volume->Session); volume->SessionLive = FALSE; }
    if (volume->Thread) {
        InterlockedExchange(&volume->ThreadStopping, 1); KeSetEvent(&volume->QueueReady, IO_NO_INCREMENT, FALSE);
        ZwWaitForSingleObject(volume->Thread, FALSE, NULL); ZwClose(volume->Thread); volume->Thread = NULL;
    }
    DeleteBacking(volume);
    DigestClose(volume);
    if (volume->TransferMdl) { ExFreePoolWithTag(volume->TransferMdl, TAG); volume->TransferMdl = NULL; }
    if (volume->TransferIrp) { IoFreeIrp(volume->TransferIrp); volume->TransferIrp = NULL; }
    if (volume->ScratchAllocation) { ExFreePoolWithTag(volume->ScratchAllocation, TAG); volume->ScratchAllocation = NULL; volume->Scratch = NULL; }
    if (volume->Slots) { ExFreePoolWithTag(volume->Slots, TAG); volume->Slots = NULL; }
    volume->Capacity = volume->SourceLength = 0; volume->SlotCount = 0;
    if (gActive == volume) gActive = NULL;
}

static NTSTATUS Query(PDEVICE_OBJECT device, ULONG code, PVOID input, ULONG inBytes, PVOID output, ULONG outBytes, PULONG_PTR returned) {
    KEVENT event; IO_STATUS_BLOCK iosb; PIRP irp; NTSTATUS status;
    *returned = 0; RtlZeroMemory(&iosb, sizeof(iosb)); KeInitializeEvent(&event, NotificationEvent, FALSE);
    irp = IoBuildDeviceIoControlRequest(code, device, input, inBytes, output, outBytes, FALSE, &event, &iosb);
    if (!irp) return STATUS_INSUFFICIENT_RESOURCES;
    status = IoCallDriver(device, irp);
    if (status == STATUS_PENDING) { KeWaitForSingleObject(&event, Executive, KernelMode, FALSE, NULL); status = iosb.Status; }
    *returned = iosb.Information; return status;
}
static NTSTATUS LabDisk(PDEVICE_OBJECT device, PULONG number) {
    STORAGE_PROPERTY_QUERY query; UCHAR bytes[1024]; ULONG_PTR returned; NTSTATUS status;
    STORAGE_DEVICE_DESCRIPTOR *descriptor = (STORAGE_DEVICE_DESCRIPTOR*)bytes; VOLUME_DISK_EXTENTS extents;
    RtlZeroMemory(&query, sizeof(query)); RtlZeroMemory(bytes, sizeof(bytes)); query.PropertyId = StorageDeviceProperty;
    status = Query(device, IOCTL_STORAGE_QUERY_PROPERTY, &query, sizeof(query), bytes, sizeof(bytes), &returned);
    if (!NT_SUCCESS(status) || returned < sizeof(*descriptor) || descriptor->RemovableMedia
        || descriptor->BusType != BusTypeFileBackedVirtual) return STATUS_NOT_SUPPORTED;
    RtlZeroMemory(&extents, sizeof(extents));
    status = Query(device, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, NULL, 0, &extents, sizeof(extents), &returned);
    if (!NT_SUCCESS(status) || returned < sizeof(extents) || extents.NumberOfDiskExtents != 1
        || extents.Extents[0].StartingOffset.QuadPart < 0 || extents.Extents[0].ExtentLength.QuadPart <= 0) return STATUS_NOT_SUPPORTED;
    *number = extents.Extents[0].DiskNumber; return STATUS_SUCCESS;
}
/* Use the file's actual filesystem device; never IoGetBaseFileSystemDeviceObject,
 * which is reserved for the operating system. The returned reference is owned. */
static NTSTATUS StorageDevice(PFILE_OBJECT file, PDEVICE_OBJECT *device) {
    *device = NULL;
    if (file->DeviceObject->DeviceType == FILE_DEVICE_DISK_FILE_SYSTEM)
        return IoGetDiskDeviceObject(file->DeviceObject, device);
    if (file->DeviceObject->DeviceType != FILE_DEVICE_DISK) return STATUS_NOT_SUPPORTED;
    *device = file->DeviceObject; ObReferenceObject(*device); return STATUS_SUCCESS;
}
static NTSTATUS Ntfs(HANDLE handle) {
    IO_STATUS_BLOCK iosb; UCHAR bytes[sizeof(FILE_FS_ATTRIBUTE_INFORMATION) + 32];
    FILE_FS_ATTRIBUTE_INFORMATION *fs = (FILE_FS_ATTRIBUTE_INFORMATION*)bytes; NTSTATUS status;
    RtlZeroMemory(bytes, sizeof(bytes));
    status = ZwQueryVolumeInformationFile(handle, &iosb, bytes, sizeof(bytes), FileFsAttributeInformation);
    return NT_SUCCESS(status) && fs->FileSystemNameLength == 8 && RtlCompareMemory(fs->FileSystemName, L"NTFS", 8) == 8
        ? STATUS_SUCCESS : STATUS_NOT_SUPPORTED;
}
static NTSTATUS WaitMetadata(HANDLE handle, NTSTATUS status, PIO_STATUS_BLOCK iosb) {
    if (status != STATUS_PENDING) return status;
    status = ZwWaitForSingleObject(handle, FALSE, NULL);
    return NT_SUCCESS(status) ? iosb->Status : status;
}
#if !SHDD_RAW_STORE
static NTSTATUS OpenParent(PCWSTR path, ULONGLONG capacity, HANDLE *directory, PULONG disk) {
    WCHAR parent[520]; ULONG split = 0, count = 0, i; UNICODE_STRING name; OBJECT_ATTRIBUTES attributes;
    IO_STATUS_BLOCK iosb; PFILE_OBJECT file = NULL; PDEVICE_OBJECT device = NULL; NTSTATUS status;
    FILE_FS_SIZE_INFORMATION size; ULONGLONG unit, total, available, reserve;
    *directory = NULL;
    while (path[count]) { if (path[count] == L'\\') split = count; count++; }
    if (!split || !path[split + 1] || path[split + 1] == L'.') return STATUS_OBJECT_NAME_INVALID;
    for (i = split + 1; i < count; i++) if (path[i] < L' ' || path[i] == L':') return STATUS_OBJECT_NAME_INVALID;
    RtlCopyMemory(parent, path, (split + 1) * sizeof(WCHAR)); parent[split + 1] = 0; RtlInitUnicodeString(&name, parent);
    InitializeObjectAttributes(&attributes, &name, OBJ_CASE_INSENSITIVE | OBJ_KERNEL_HANDLE | OBJ_DONT_REPARSE, NULL, NULL);
    status = ZwOpenFile(directory, FILE_READ_ATTRIBUTES | SYNCHRONIZE, &attributes, &iosb, FILE_SHARE_READ | FILE_SHARE_WRITE,
        FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT);
    if (!NT_SUCCESS(status)) return status;
    status = Ntfs(*directory); if (!NT_SUCCESS(status)) goto done;
    status = ObReferenceObjectByHandle(*directory, FILE_READ_ATTRIBUTES, *IoFileObjectType, KernelMode, (PVOID*)&file, NULL);
    if (!NT_SUCCESS(status)) goto done;
    status = StorageDevice(file, &device); if (!NT_SUCCESS(status)) goto done;
    if (device->AlignmentRequirement > 4095) { status = STATUS_NOT_SUPPORTED; goto done; }
    status = LabDisk(device, disk); if (!NT_SUCCESS(status)) goto done;
    status = ZwQueryVolumeInformationFile(*directory, &iosb, &size, sizeof(size), FileFsSizeInformation);
    if (!NT_SUCCESS(status)) goto done;
    unit = (ULONGLONG)size.SectorsPerAllocationUnit * size.BytesPerSector;
    if (!unit || size.BytesPerSector > 4096 || 4096 % size.BytesPerSector || size.TotalAllocationUnits.QuadPart <= 0
        || size.AvailableAllocationUnits.QuadPart < 0 || size.AvailableAllocationUnits.QuadPart > size.TotalAllocationUnits.QuadPart
        || (ULONGLONG)size.TotalAllocationUnits.QuadPart > MAXULONGLONG / unit) { status = STATUS_INVALID_PARAMETER; goto done; }
    total = (ULONGLONG)size.TotalAllocationUnits.QuadPart * unit; available = (ULONGLONG)size.AvailableAllocationUnits.QuadPart * unit;
    reserve = max(total / 10, 64ULL * 1024 * 1024);
    if (available < reserve || capacity > available - reserve) status = STATUS_DISK_FULL;
done:
    if (device) ObDereferenceObject(device); if (file) ObDereferenceObject(file);
    if (!NT_SUCCESS(status)) { ZwClose(*directory); *directory = NULL; } return status;
}
#else
#include "direct_source.inc"
#include "raw_backend.inc"
#endif
static NTSTATUS Configure(SHDD_ATTACH *request) {
    HANDLE source = NULL; PFILE_OBJECT file = NULL; PDEVICE_OBJECT storage = NULL, base = NULL;
    VOLUME *volume = NULL; PLIST_ENTRY link; UNICODE_STRING name; OBJECT_ATTRIBUTES attributes;
    IO_STATUS_BLOCK iosb; NTSTATUS status; ULONG sourceDisk, backingDisk; ULONG_PTR returned;
    GET_LENGTH_INFORMATION length;
#if !SHDD_RAW_STORE
    HANDLE parent = NULL; ULONG i=0,split=0; LARGE_INTEGER capacity;
    FILE_END_OF_FILE_INFORMATION eof; FILE_ATTRIBUTE_TAG_INFORMATION tags;
#endif
#if SHDD_RAW_STORE
    RtlZeroMemory(&gDiagnostic,sizeof(gDiagnostic));
#endif
    ConfigStep(1);
    if (!SHDD_ENABLE_EXPERIMENTAL_ATTACH) return STATUS_NOT_SUPPORTED;
    if (gActive || request->Version != SHDD_PNP_PROTOCOL_VERSION || request->DisposableVolumeAcknowledged != (SHDD_DIRECT_VM_BUILD ? SHDD_VM_ACK : SHDD_DATA_ACK)
        || !ShddCapacityValid(request->CapacityBytes) || !ShddWirePathValid(request->SourceDevice, 260, FALSE)
        || !ShddWirePathValid(request->BackingFile, 520, SHDD_RAW_STORE ? FALSE : TRUE)) return STATUS_INVALID_PARAMETER;
    RtlInitUnicodeString(&name, request->SourceDevice);
    InitializeObjectAttributes(&attributes, &name, OBJ_CASE_INSENSITIVE | OBJ_KERNEL_HANDLE | OBJ_DONT_REPARSE, NULL, NULL);
    status = ZwOpenFile(&source, FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE, &attributes, &iosb,
        FILE_SHARE_READ | FILE_SHARE_WRITE, FILE_SYNCHRONOUS_IO_NONALERT);
    ConfigStep(2);
    if (!NT_SUCCESS(status)) goto done;
    ConfigStep(3);status = Ntfs(source); if (!NT_SUCCESS(status)) goto done;
    ConfigStep(4);
    status = ObReferenceObjectByHandle(source, FILE_READ_DATA, *IoFileObjectType, KernelMode, (PVOID*)&file, NULL);
    if (!NT_SUCCESS(status)) goto done;
    ConfigStep(5);
#if SHDD_RAW_STORE
    gDiagnostic.SourceFileFlags=file->Flags;gDiagnostic.SourceDeviceType=file->DeviceObject->DeviceType;
#endif
    if (!(file->Flags & FO_VOLUME_OPEN)) { status = STATUS_NOT_SUPPORTED; goto done; }
    ConfigStep(6);
    status = StorageDevice(file, &storage); if (!NT_SUCCESS(status)) goto done;
    base = IoGetDeviceAttachmentBaseRef(storage);
    ConfigStep(7);
    for (link = gVolumes.Flink; link != &gVolumes; link = link->Flink) {
        VOLUME *candidate = CONTAINING_RECORD(link, VOLUME, DeviceLink);
        if (candidate->Pdo == base) { volume = candidate; break; }
    }
    ConfigStep(8);
#if SHDD_RAW_STORE
    if(volume){gDiagnostic.LowerFlags=volume->Lower->Flags;gDiagnostic.PnpState=volume->PnpState;}
#endif
    if (!volume || !ShddPnpConfigurable(ShddAtomicRead(&volume->PnpState)) || !(volume->Lower->Flags & DO_DIRECT_IO)
        || volume->Lower->AlignmentRequirement > 4095) { volume = NULL; status = STATUS_NOT_SUPPORTED; goto done; }
    status = IoAcquireRemoveLock(&volume->RemoveLock, request);
    if (!NT_SUCCESS(status)) { volume = NULL; goto done; }
    ConfigStep(9);status = DirectSource(volume, request, &sourceDisk); if (!NT_SUCCESS(status)) goto done;
    ConfigStep(10);
    status = Query(volume->Lower, IOCTL_DISK_GET_LENGTH_INFO, NULL, 0, &length, sizeof(length), &returned);
    if (!NT_SUCCESS(status) || returned < sizeof(length) || length.Length.QuadPart < SHDD_BLOCK_BYTES || (ULONGLONG)length.Length.QuadPart != request->SourceBytes) { status = STATUS_NOT_SUPPORTED; goto done; }
#if SHDD_RAW_STORE
    volume->SourceLength=length.Length.QuadPart;
    ConfigStep(11);status=RawOpen(volume,request,source,sourceDisk,&backingDisk);if(!NT_SUCCESS(status))goto done;
    volume->SlotCount=volume->StoreHeader.SlotCount;
#else
    status = OpenParent(request->BackingFile, request->CapacityBytes, &parent, &backingDisk);
    if (!NT_SUCCESS(status)) goto done;
    if (sourceDisk == backingDisk) { status = STATUS_INVALID_PARAMETER; goto done; }
    while (request->BackingFile[i]) { if (request->BackingFile[i] == L'\\') split = i; i++; }
    RtlInitUnicodeString(&name, request->BackingFile + split + 1);
    InitializeObjectAttributes(&attributes, &name, OBJ_CASE_INSENSITIVE | OBJ_KERNEL_HANDLE | OBJ_DONT_REPARSE, parent, NULL);
    capacity.QuadPart = (LONGLONG)request->CapacityBytes;
    status = ZwCreateFile(&volume->Backing, SHDD_BACKING_ACCESS, &attributes, &iosb, &capacity,
        FILE_ATTRIBUTE_NORMAL, 0, FILE_CREATE, FILE_NON_DIRECTORY_FILE | FILE_NO_INTERMEDIATE_BUFFERING | FILE_OPEN_REPARSE_POINT, NULL, 0);
    status = WaitMetadata(volume->Backing, status, &iosb);
    if (!NT_SUCCESS(status)) goto done;
    status = ZwQueryInformationFile(volume->Backing, &iosb, &tags, sizeof(tags), FileAttributeTagInformation);
    status = WaitMetadata(volume->Backing, status, &iosb);
    if (!NT_SUCCESS(status) || (tags.FileAttributes & (FILE_ATTRIBUTE_COMPRESSED | FILE_ATTRIBUTE_ENCRYPTED | FILE_ATTRIBUTE_SPARSE_FILE | FILE_ATTRIBUTE_REPARSE_POINT))) {
        status = STATUS_NOT_SUPPORTED; goto done;
    }
    eof.EndOfFile = capacity; status = ZwSetInformationFile(volume->Backing, &iosb, &eof, sizeof(eof), FileEndOfFileInformation);
    status = WaitMetadata(volume->Backing, status, &iosb);
    if (!NT_SUCCESS(status)) goto done;
    status = ObReferenceObjectByHandle(volume->Backing, FILE_READ_DATA | FILE_WRITE_DATA, *IoFileObjectType, KernelMode, (PVOID*)&volume->BackingFile, NULL);
    if (!NT_SUCCESS(status)) goto done;
    if (IoGetRelatedDeviceObject(volume->BackingFile)->Flags & DO_BUFFERED_IO) { status = STATUS_NOT_SUPPORTED; goto done; }
    volume->SlotCount = (ULONG)(request->CapacityBytes / SHDD_BLOCK_BYTES);
#endif
    ConfigStep(12);volume->Slots = ExAllocatePool2(POOL_FLAG_NON_PAGED, sizeof(SLOT) * volume->SlotCount, TAG);
    if (!volume->Slots) { status = STATUS_INSUFFICIENT_RESOURCES; goto done; }
    RtlZeroMemory(volume->Slots, sizeof(SLOT) * volume->SlotCount);
#if SHDD_RAW_STORE
    ConfigStep(13);status=RawLoad(volume);if(!NT_SUCCESS(status))goto done;ConfigStep(14);
#endif
    /* No per-request pool/IRP/MDL allocation in the dedicated reader. A failed
     * reservation fails attachment before requests can enter this queue. */
    volume->ScratchAllocation = ExAllocatePool2(POOL_FLAG_NON_PAGED, SHDD_BLOCK_BYTES + 4095, TAG);
    if (!volume->ScratchAllocation) { status = STATUS_INSUFFICIENT_RESOURCES; goto done; }
    volume->Scratch = (UCHAR*)(((ULONG_PTR)volume->ScratchAllocation + 4095) & ~(ULONG_PTR)4095);
    /* Own the MDL storage directly. MmInitializeMdl resets flags, so reusing an
     * IoAllocateMdl lookaside allocation here would lose its allocator flags. */
    volume->TransferMdl = ExAllocatePool2(POOL_FLAG_NON_PAGED,
        MmSizeOfMdl(volume->Scratch, SHDD_BLOCK_BYTES + 4095), TAG);
    {
        CCHAR count = volume->Lower->StackSize;
#if SHDD_RAW_STORE
        if (volume->RawDevice->StackSize > count) count = volume->RawDevice->StackSize;
#endif
        volume->TransferIrp = IoAllocateIrp(count, FALSE);
    }
    if (!volume->TransferMdl || !volume->TransferIrp) { status = STATUS_INSUFFICIENT_RESOURCES; goto done; }
    ConfigStep(36);status=DigestOpen(volume);if(!NT_SUCCESS(status))goto done;
    /* Epoch is never reset while forwarded mutations are in flight. */
    if (ShddAtomicRead(&volume->Epoch.ActiveMutations) || ShddGeneration(&volume->Epoch) == 0x7fffffffffffffffLL) { status = STATUS_DEVICE_BUSY; goto done; }
    ShddInvalidate(&volume->Epoch); InterlockedExchange(&volume->Epoch.Disabled, 0);
    ExReInitializeRundownProtection(&volume->Session); volume->SessionLive = TRUE;
    volume->Capacity = (ULONGLONG)volume->SlotCount*SHDD_BLOCK_BYTES; volume->SourceLength = length.Length.QuadPart;
    volume->SourceDisk = sourceDisk; volume->BackingDisk = backingDisk;
    InterlockedExchange(&volume->Paused, 0); InterlockedExchange(&volume->ThreadStopping, 0);
    KeClearEvent(&volume->QueueReady);
    InitializeObjectAttributes(&attributes, NULL, OBJ_KERNEL_HANDLE, NULL, NULL);
    status = PsCreateSystemThread(&volume->Thread, THREAD_ALL_ACCESS, &attributes, NULL, NULL, Reader, volume);
    if (!NT_SUCCESS(status)) goto done;
    if (!ShddPnpConfigurable(ShddAtomicRead(&volume->PnpState)) || ShddAtomicRead(&volume->SpecialUsage)) { status = STATUS_DEVICE_NOT_READY; goto done; }
    gActive = volume; InterlockedExchange(&volume->Enabled, 1);ConfigStep(16);
done:
#if SHDD_RAW_STORE
    gDiagnostic.Status=(ULONG)status;
    if(volume){
        gDiagnostic.ActiveMutations=(ULONG)ShddAtomicRead(&volume->Epoch.ActiveMutations);
        gDiagnostic.PendingControls=(ULONG)ShddAtomicRead(&volume->PendingControls);
        gDiagnostic.LastControlMajor=(ULONG)ShddAtomicRead(&volume->LastControlMajor);
        gDiagnostic.LastControlCode=(ULONG)ShddAtomicRead(&volume->LastControlCode);
    }
#endif
    if (volume) { if (!NT_SUCCESS(status)) DisableSession(volume); IoReleaseRemoveLock(&volume->RemoveLock, request); }
    if (base) ObDereferenceObject(base); if (storage) ObDereferenceObject(storage); if (file) ObDereferenceObject(file);
#if !SHDD_RAW_STORE
    if (parent) ZwClose(parent);
#endif
    if (source) ZwClose(source); return status;
}
_Use_decl_annotations_
static NTSTATUS SignalDone(PDEVICE_OBJECT device, PIRP irp, PVOID context) {
    UNREFERENCED_PARAMETER(device); UNREFERENCED_PARAMETER(irp);
    if (context) KeSetEvent((PKEVENT)context, IO_NO_INCREMENT, FALSE);
    return STATUS_MORE_PROCESSING_REQUIRED;
}
static NTSTATUS Start(VOLUME *volume, PIRP irp) {
    KEVENT event; NTSTATUS status;
    KeInitializeEvent(&event, NotificationEvent, FALSE); IoCopyCurrentIrpStackLocationToNext(irp);
    IoSetCompletionRoutine(irp, SignalDone, &event, TRUE, TRUE, TRUE); status = IoCallDriver(volume->Lower, irp);
    if (status == STATUS_PENDING) KeWaitForSingleObject(&event, Executive, KernelMode, FALSE, NULL);
    status = irp->IoStatus.Status;
    if (NT_SUCCESS(status)) InterlockedOr(&volume->PnpState, SHDD_PNP_STARTED);
    if (NT_SUCCESS(status)) InterlockedAnd(&volume->PnpState, ~(SHDD_PNP_STOPPED | SHDD_PNP_POWER_CHANGED));
    IoReleaseRemoveLock(&volume->RemoveLock, irp); Complete(irp, status, irp->IoStatus.Information); return status;
}
_Use_decl_annotations_
static NTSTATUS UsageDone(PDEVICE_OBJECT device, PIRP irp, PVOID context) {
    USAGE_CHANGE *change=context; VOLUME *volume;
    SHDD_USAGE_COUNTS counts;
    if(!change)return ForwardDone(device,irp,NULL);
    volume=change->Volume;
    if(!NT_SUCCESS(irp->IoStatus.Status)) {
        InterlockedExchange(&volume->PagingFiles,(LONG)change->Before.Paging);
        InterlockedExchange(&volume->HibernationFiles,(LONG)change->Before.Hibernation);
        InterlockedExchange(&volume->DumpFiles,(LONG)change->Before.Dump);
    }
    counts.Paging=(ULONG)ShddAtomicRead(&volume->PagingFiles);
    counts.Hibernation=(ULONG)ShddAtomicRead(&volume->HibernationFiles);
    counts.Dump=(ULONG)ShddAtomicRead(&volume->DumpFiles);
    InterlockedExchange(&volume->SpecialUsage,ShddUsageBlocksCache(&counts));
    if(ShddUsageBlocksCache(&counts))InterlockedOr(&volume->PnpState,SHDD_PNP_SPECIAL_FILE);
    else InterlockedAnd(&volume->PnpState,~SHDD_PNP_SPECIAL_FILE);
    if(!ShddUsagePresent(&counts) && !volume->BackingPagingPath && (volume->Lower->Flags&DO_POWER_PAGABLE)
        && !(volume->Self->Flags&DO_POWER_INRUSH))
        InterlockedOr((volatile LONG*)&volume->Self->Flags,DO_POWER_PAGABLE);
    else InterlockedAnd((volatile LONG*)&volume->Self->Flags,~DO_POWER_PAGABLE);
    IoInvalidateDeviceState(volume->Pdo);
    ExFreePoolWithTag(change,TAG);
    KeSetEvent(&volume->UsageGate,IO_NO_INCREMENT,FALSE);
    return ForwardDone(device,irp,volume);
}
static BOOLEAN HasSpecialFiles(VOLUME *volume) {
    return ShddAtomicRead(&volume->PagingFiles)>0 || ShddAtomicRead(&volume->HibernationFiles)>0
        || ShddAtomicRead(&volume->DumpFiles)>0;
}
_Use_decl_annotations_
static NTSTATUS UsageQueryDone(PDEVICE_OBJECT device,PIRP irp,PVOID context) {
    VOLUME *volume=context;
    if(NT_SUCCESS(irp->IoStatus.Status)&&HasSpecialFiles(volume))
        irp->IoStatus.Information|=PNP_DEVICE_NOT_DISABLEABLE;
    return ForwardDone(device,irp,context);
}
static NTSTATUS Usage(VOLUME *volume,PIRP irp,PIO_STACK_LOCATION stack) {
    SHDD_USAGE_COUNTS before,after;USAGE_CHANGE *change;NTSTATUS status;
    status=KeWaitForSingleObject(&volume->UsageGate,Executive,KernelMode,FALSE,NULL);
    if(!NT_SUCCESS(status)){IoReleaseRemoveLock(&volume->RemoveLock,irp);return Complete(irp,status,0);}
    before.Paging=(ULONG)ShddAtomicRead(&volume->PagingFiles);
    before.Hibernation=(ULONG)ShddAtomicRead(&volume->HibernationFiles);
    before.Dump=(ULONG)ShddAtomicRead(&volume->DumpFiles);
    if(!ShddUsageNext(&before,(ULONG)stack->Parameters.UsageNotification.Type,
            stack->Parameters.UsageNotification.InPath,&after)) {
        KeSetEvent(&volume->UsageGate,IO_NO_INCREMENT,FALSE);
        IoReleaseRemoveLock(&volume->RemoveLock,irp);return Complete(irp,STATUS_NOT_SUPPORTED,0);
    }
    change=ExAllocatePool2(POOL_FLAG_NON_PAGED,sizeof(*change),TAG);
    if(!change){KeSetEvent(&volume->UsageGate,IO_NO_INCREMENT,FALSE);IoReleaseRemoveLock(&volume->RemoveLock,irp);return Complete(irp,STATUS_INSUFFICIENT_RESOURCES,0);}
    change->Volume=volume;change->Before=before;
    if(ShddUsageBlocksCache(&after)) {
        InterlockedExchange(&volume->SpecialUsage,1);
        InterlockedOr(&volume->PnpState,SHDD_PNP_SPECIAL_FILE);ShddDisable(&volume->Epoch);
    }
    InterlockedExchange(&volume->PagingFiles,(LONG)after.Paging);
    InterlockedExchange(&volume->HibernationFiles,(LONG)after.Hibernation);
    InterlockedExchange(&volume->DumpFiles,(LONG)after.Dump);
    if(ShddUsagePresent(&after) || volume->BackingPagingPath)InterlockedAnd((volatile LONG*)&volume->Self->Flags,~DO_POWER_PAGABLE);
    else if((volume->Lower->Flags&DO_POWER_PAGABLE)&&!(volume->Self->Flags&DO_POWER_INRUSH))
        InterlockedOr((volatile LONG*)&volume->Self->Flags,DO_POWER_PAGABLE);
    irp->IoStatus.Status=STATUS_SUCCESS;
    IoCopyCurrentIrpStackLocationToNext(irp);IoSetCompletionRoutine(irp,UsageDone,change,TRUE,TRUE,TRUE);
    return IoCallDriver(volume->Lower,irp);
}
_Use_decl_annotations_
static NTSTATUS CancelPnpDone(PDEVICE_OBJECT device, PIRP irp, PVOID context) {
    VOLUME *volume = context;
    if (volume && NT_SUCCESS(irp->IoStatus.Status)) InterlockedAnd(&volume->PnpState, ~SHDD_PNP_STOPPED);
    return ForwardDone(device, irp, context);
}
_Use_decl_annotations_
static NTSTATUS PowerReadyDone(PDEVICE_OBJECT device, PIRP irp, PVOID context) {
    VOLUME *volume = context;
    if (volume && NT_SUCCESS(irp->IoStatus.Status)) InterlockedAnd(&volume->PnpState, ~SHDD_PNP_POWER_CHANGED);
    /* Epoch.Disabled stays sticky: resuming power never revives old cache data. */
    return ForwardDone(device, irp, context);
}
static NTSTATUS Pnp(VOLUME *volume, PIRP irp) {
    PIO_STACK_LOCATION stack = IoGetCurrentIrpStackLocation(irp); NTSTATUS status;
    if (stack->MinorFunction == IRP_MN_START_DEVICE) return Start(volume, irp);
    if(stack->MinorFunction==IRP_MN_DEVICE_USAGE_NOTIFICATION)return Usage(volume,irp,stack);
    if((stack->MinorFunction==IRP_MN_QUERY_STOP_DEVICE || stack->MinorFunction==IRP_MN_QUERY_REMOVE_DEVICE)
        &&HasSpecialFiles(volume)) {
        IoReleaseRemoveLock(&volume->RemoveLock,irp);return Complete(irp,STATUS_DEVICE_BUSY,0);
    }
    if(stack->MinorFunction==IRP_MN_QUERY_PNP_DEVICE_STATE) {
        IoCopyCurrentIrpStackLocationToNext(irp);IoSetCompletionRoutine(irp,UsageQueryDone,volume,TRUE,TRUE,TRUE);
        return IoCallDriver(volume->Lower,irp);
    }
    if (stack->MinorFunction == IRP_MN_CANCEL_STOP_DEVICE || stack->MinorFunction == IRP_MN_CANCEL_REMOVE_DEVICE) {
        IoCopyCurrentIrpStackLocationToNext(irp); IoSetCompletionRoutine(irp, CancelPnpDone, volume, TRUE, TRUE, TRUE);
        return IoCallDriver(volume->Lower, irp);
    }
    /* A cancelled stop/remove keeps the session disabled until explicit configure. */
    if (stack->MinorFunction == IRP_MN_QUERY_STOP_DEVICE || stack->MinorFunction == IRP_MN_STOP_DEVICE
        || stack->MinorFunction == IRP_MN_QUERY_REMOVE_DEVICE || stack->MinorFunction == IRP_MN_SURPRISE_REMOVAL
        || stack->MinorFunction == IRP_MN_REMOVE_DEVICE) {
        InterlockedOr(&volume->PnpState, stack->MinorFunction == IRP_MN_SURPRISE_REMOVAL ? SHDD_PNP_SURPRISE
            : stack->MinorFunction == IRP_MN_REMOVE_DEVICE ? SHDD_PNP_REMOVED : SHDD_PNP_STOPPED);
        ShddDisable(&volume->Epoch);
    }
    if (stack->MinorFunction != IRP_MN_REMOVE_DEVICE) return Forward(volume, irp);
    /* Forward REMOVE before waiting for child/forwarded I/O: lower drivers may
     * need this notification to complete outstanding requests. The reader drains
     * its queue as bypasses while disabled. Do not hold gConfiguration here. */
    InterlockedExchange(&volume->Enabled, 0);
    irp->IoStatus.Status = STATUS_SUCCESS; IoSkipCurrentIrpStackLocation(irp);
    status = IoCallDriver(volume->Lower, irp);
    IoReleaseRemoveLockAndWait(&volume->RemoveLock, irp);
    KeWaitForSingleObject(&gConfiguration, Executive, KernelMode, FALSE, NULL);
    DisableSession(volume); RemoveEntryList(&volume->DeviceLink); gVolumeCount--;
    KeReleaseMutex(&gConfiguration, FALSE);
    IoDetachDevice(volume->Lower); IoDeleteDevice(volume->Self); return status;
}
static NTSTATUS Control(PIRP irp) {
    PIO_STACK_LOCATION stack = IoGetCurrentIrpStackLocation(irp); NTSTATUS status = STATUS_INVALID_DEVICE_REQUEST; ULONG_PTR bytes = 0;
    ULONG code = stack->Parameters.DeviceIoControl.IoControlCode;
    if (stack->MajorFunction == IRP_MJ_CREATE || stack->MajorFunction == IRP_MJ_CLOSE || stack->MajorFunction == IRP_MJ_CLEANUP)
        return Complete(irp, STATUS_SUCCESS, 0);
    if (stack->MajorFunction != IRP_MJ_DEVICE_CONTROL || KeGetCurrentIrql() != PASSIVE_LEVEL) return Complete(irp, status, 0);
    KeWaitForSingleObject(&gConfiguration, Executive, KernelMode, FALSE, NULL);
#if SHDD_RAW_STORE
    if(code==SHDD_RAW_DIAGNOSTIC_IOCTL&&stack->Parameters.DeviceIoControl.OutputBufferLength==sizeof(gDiagnostic)){
        RtlCopyMemory(irp->AssociatedIrp.SystemBuffer,&gDiagnostic,sizeof(gDiagnostic));
        KeReleaseMutex(&gConfiguration,FALSE);return Complete(irp,STATUS_SUCCESS,sizeof(gDiagnostic));
    }
#endif
    if (code == SHDD_PNP_STATUS_IOCTL && stack->Parameters.DeviceIoControl.OutputBufferLength == sizeof(SHDD_PNP_STATUS)) {
        SHDD_PNP_STATUS *out = irp->AssociatedIrp.SystemBuffer; VOLUME *v = gActive;
        RtlZeroMemory(out, sizeof(*out)); out->Cache.Version = SHDD_PNP_PROTOCOL_VERSION;
        out->Cache.ExperimentalAttachEnabled = SHDD_ENABLE_EXPERIMENTAL_ATTACH;
        out->DirectVmBuild = SHDD_DIRECT_VM_BUILD ? 1u : 0u; out->PolicyVersion = 3u;
        out->PagingPassthrough=1;
        out->Cache.QueueLimit = SHDD_MAX_PENDING_READS; out->Cache.MaxReadBytes = SHDD_MAX_READ_BYTES; out->VolumeCount = gVolumeCount;
        if (v) {
            out->Cache.Attached = 1; out->Cache.Paused = ShddAtomicRead(&v->Paused); out->Cache.CacheDisabled = ShddAtomicRead(&v->Epoch.Disabled);
            out->Cache.Hits = v->Hits; out->Cache.Misses = v->Misses; out->Cache.SourceReadBytes = v->SourceReadBytes;
            out->Cache.SsdReadBytes = v->SsdReadBytes; out->Cache.FillBytes = v->FillBytes; out->Cache.Evictions = v->Evictions;
            out->Cache.Errors = v->Errors; out->Cache.ReadBypasses = v->Bypasses; out->Cache.Generation = ShddGeneration(&v->Epoch);
            out->Cache.CapacityBytes = v->Capacity; out->Cache.SourceLength = v->SourceLength;
            out->Cache.ActiveMutations = ShddAtomicRead(&v->Epoch.ActiveMutations); out->Cache.PendingReads = ShddAtomicRead(&v->PendingReads);
            out->SourceDiskNumber = v->SourceDisk; out->BackingDiskNumber = v->BackingDisk; out->PnpState = ShddAtomicRead(&v->PnpState);
            out->CancelledReads = v->CancelledReads; out->ChildCancellations = v->ChildCancellations;
            out->PagingFiles=(ULONG)ShddAtomicRead(&v->PagingFiles);
            out->HibernationFiles=(ULONG)ShddAtomicRead(&v->HibernationFiles);
            out->DumpFiles=(ULONG)ShddAtomicRead(&v->DumpFiles);
            out->PagingBypasses=InterlockedCompareExchange64(&v->PagingBypasses,0,0);
#if SHDD_RAW_STORE
            out->RawBackend=1;out->StoreLocked=v->StoreLocked;out->AllocatedBytes=v->StoreHeader.TotalBytes;
            out->PagingPassthrough=v->BackingPagingPath ? 0u : 1u;
            out->ImportedRecords=v->ImportedRecords;out->RevalidatedRecords=v->RevalidatedRecords;
            out->ValidationReadBytes=v->ValidationReadBytes;out->PagingHits=v->PagingHits;
#endif
        }
        status = STATUS_SUCCESS; bytes = sizeof(*out);
    } else if (code == SHDD_PNP_CONFIGURE_IOCTL && stack->Parameters.DeviceIoControl.InputBufferLength == sizeof(SHDD_ATTACH)) {
        status = Configure(irp->AssociatedIrp.SystemBuffer);
    } else if (code == SHDD_PNP_DISABLE_IOCTL && !stack->Parameters.DeviceIoControl.InputBufferLength) {
        if (gActive) DisableSession(gActive); status = STATUS_SUCCESS;
    } else if (gActive && code == SHDD_PNP_PAUSE_IOCTL && stack->Parameters.DeviceIoControl.InputBufferLength == sizeof(ULONG)
        && *(ULONG*)irp->AssociatedIrp.SystemBuffer <= 1) {
        InterlockedExchange(&gActive->Paused, *(ULONG*)irp->AssociatedIrp.SystemBuffer); ShddInvalidate(&gActive->Epoch); status = STATUS_SUCCESS;
    } else if (gActive && code == SHDD_PNP_CLEAR_IOCTL && !stack->Parameters.DeviceIoControl.InputBufferLength) {
#if SHDD_RAW_STORE
        status=RawClear(gActive);
#else
        ShddInvalidate(&gActive->Epoch); status = STATUS_SUCCESS;
#endif
    }
    KeReleaseMutex(&gConfiguration, FALSE); return Complete(irp, status, bytes);
}
_Use_decl_annotations_
static NTSTATUS Dispatch(PDEVICE_OBJECT device, PIRP irp) {
    VOLUME *volume = device->DeviceExtension; PIO_STACK_LOCATION stack = IoGetCurrentIrpStackLocation(irp); NTSTATUS status;
    if (volume->Control) return Control(irp);
    status = IoAcquireRemoveLock(&volume->RemoveLock, irp); if (!NT_SUCCESS(status)) return Complete(irp, status, 0);
    if (stack->MajorFunction == IRP_MJ_PNP) return Pnp(volume, irp);
    if (stack->MajorFunction == IRP_MJ_POWER && stack->MinorFunction == IRP_MN_SET_POWER) {
        BOOLEAN ready = (stack->Parameters.Power.Type == DevicePowerState && stack->Parameters.Power.State.DeviceState == PowerDeviceD0)
            || (stack->Parameters.Power.Type == SystemPowerState && stack->Parameters.Power.State.SystemState == PowerSystemWorking);
        if (ready) {
            IoCopyCurrentIrpStackLocationToNext(irp); IoSetCompletionRoutine(irp, PowerReadyDone, volume, TRUE, TRUE, TRUE);
            return IoCallDriver(volume->Lower, irp);
        }
        InterlockedOr(&volume->PnpState, SHDD_PNP_POWER_CHANGED); ShddDisable(&volume->Epoch);
    }
    if (stack->MajorFunction == IRP_MJ_SHUTDOWN) {
        InterlockedOr(&volume->PnpState, SHDD_PNP_POWER_CHANGED); ShddDisable(&volume->Epoch);
    }
    /* A cached paging read requires an enrolled backing storage path. Writes
     * always retain normal source invalidation and forwarding semantics. */
    if(stack->MajorFunction==IRP_MJ_READ && (irp->Flags&(IRP_PAGING_IO|IRP_SYNCHRONOUS_PAGING_IO))
#if SHDD_RAW_STORE
        && !volume->BackingPagingPath
#endif
    ) {
        InterlockedIncrement64(&volume->PagingBypasses);InterlockedIncrement64(&volume->Bypasses);
        return Forward(volume,irp);
    }
    if (stack->MajorFunction == IRP_MJ_READ
        && !ShddAtomicRead(&volume->BackingIo)
        && ShddPnpReadable(ShddAtomicRead(&volume->PnpState), ShddAtomicRead(&volume->Enabled), ShddAtomicRead(&volume->Paused), &volume->Epoch)
        && irp->MdlAddress && !irp->MdlAddress->Next
        && ExAcquireRundownProtection(&volume->Session)) {
        /* Read session-owned metadata only after acquiring its rundown. */
        if (ShddEligibleStorageRead(stack->Parameters.Read.ByteOffset.QuadPart, stack->Parameters.Read.Length, volume->SourceLength,
                MmGetMdlByteCount(irp->MdlAddress), (irp->Flags & (IRP_PAGING_IO | IRP_SYNCHRONOUS_PAGING_IO)) != 0,
#if SHDD_RAW_STORE
                volume->StoreLocked)
#else
                FALSE)
#endif
            && ShddReserveRead(&volume->PendingReads)) {
            IoMarkIrpPending(irp);
            status = IoCsqInsertIrpEx(&volume->Csq, irp, NULL, NULL);
            if (!NT_SUCCESS(status)) ReleaseRead(volume, irp, TRUE, STATUS_SUCCESS, 0);
            return STATUS_PENDING;
        }
        ExReleaseRundownProtection(&volume->Session);
    }
    if (stack->MajorFunction == IRP_MJ_READ) {
        InterlockedIncrement64(&volume->Bypasses);
        if(irp->Flags&(IRP_PAGING_IO|IRP_SYNCHRONOUS_PAGING_IO))InterlockedIncrement64(&volume->PagingBypasses);
    }
    return Forward(volume, irp);
}
_Use_decl_annotations_
static NTSTATUS AddDevice(PDRIVER_OBJECT driver, PDEVICE_OBJECT pdo) {
    PDEVICE_OBJECT device, top; VOLUME *volume; NTSTATUS status;
    status = IoCreateDevice(driver, sizeof(VOLUME), NULL, FILE_DEVICE_DISK, FILE_DEVICE_SECURE_OPEN, FALSE, &device);
    if (!NT_SUCCESS(status)) return status;
    volume = device->DeviceExtension; RtlZeroMemory(volume, sizeof(*volume)); volume->Self = device; volume->Pdo = pdo;
    volume->Epoch.Generation = 1; volume->Epoch.Disabled = 1;
    IoInitializeRemoveLock(&volume->RemoveLock, TAG, 0, 0); KeInitializeSpinLock(&volume->QueueLock);
    InitializeListHead(&volume->Queue); KeInitializeEvent(&volume->QueueReady, NotificationEvent, FALSE);
    KeInitializeEvent(&volume->UsageGate,SynchronizationEvent,TRUE);
    ExInitializeRundownProtection(&volume->Session); ExWaitForRundownProtectionRelease(&volume->Session);
    status = IoCsqInitializeEx(&volume->Csq, QueueInsert, QueueRemove, QueuePeek, QueueAcquire, QueueRelease, QueueCancel);
    if (!NT_SUCCESS(status)) { IoDeleteDevice(device); return status; }
    top = IoGetAttachedDeviceReference(pdo);
    device->Flags |= top->Flags & (DO_DIRECT_IO | DO_BUFFERED_IO | DO_POWER_PAGABLE | DO_POWER_INRUSH);
    device->Characteristics |= top->Characteristics; device->AlignmentRequirement = top->AlignmentRequirement;
    ObDereferenceObject(top);
    KeWaitForSingleObject(&gConfiguration, Executive, KernelMode, FALSE, NULL);
    status = IoAttachDeviceToDeviceStackSafe(device, pdo, &volume->Lower);
    if (NT_SUCCESS(status)) { InsertTailList(&gVolumes, &volume->DeviceLink); gVolumeCount++; device->Flags &= ~DO_DEVICE_INITIALIZING; }
    KeReleaseMutex(&gConfiguration, FALSE);
    if (!NT_SUCCESS(status)) IoDeleteDevice(device); return status;
}
_Use_decl_annotations_
static VOID Unload(PDRIVER_OBJECT driver) {
    UNREFERENCED_PARAMETER(driver);
    /* PnP owns filter deletion. This routine is called only after all removes. */
    NT_ASSERT(IsListEmpty(&gVolumes)); NT_ASSERT(gActive == NULL);
    IoDeleteSymbolicLink(&gLink); if (gControl) IoDeleteDevice(gControl);
}
_Use_decl_annotations_
NTSTATUS DriverEntry(PDRIVER_OBJECT driver, PUNICODE_STRING registry) {
#if SHDD_RAW_STORE
    UNICODE_STRING name = RTL_CONSTANT_STRING(L"\\Device\\SuperHDDCacheV8");
#else
    UNICODE_STRING name = RTL_CONSTANT_STRING(L"\\Device\\SuperHDDCachePnpLab");
#endif
    UNICODE_STRING sddl = RTL_CONSTANT_STRING(L"D:P(A;;GA;;;SY)(A;;GA;;;BA)"); NTSTATUS status; ULONG i;
    UNREFERENCED_PARAMETER(registry); KeInitializeMutex(&gConfiguration, 0); InitializeListHead(&gVolumes);
    for (i = 0; i <= IRP_MJ_MAXIMUM_FUNCTION; i++) driver->MajorFunction[i] = Dispatch;
    driver->DriverExtension->AddDevice = AddDevice; driver->DriverUnload = Unload;
    status = IoCreateDeviceSecure(driver, sizeof(VOLUME), &name, FILE_DEVICE_UNKNOWN, FILE_DEVICE_SECURE_OPEN, FALSE, &sddl, &gClass, &gControl);
    if (!NT_SUCCESS(status)) return status;
    RtlZeroMemory(gControl->DeviceExtension, sizeof(VOLUME)); ((VOLUME*)gControl->DeviceExtension)->Control = TRUE;
    gControl->Flags |= DO_BUFFERED_IO; status = IoCreateSymbolicLink(&gLink, &name);
    if (!NT_SUCCESS(status)) { IoDeleteDevice(gControl); gControl = NULL; return status; }
    gControl->Flags &= ~DO_DEVICE_INITIALIZING; return STATUS_SUCCESS;
}
