/* EXPERIMENTAL volume filter. Never load on a real data/system disk.
 * The shipping configuration disables attach. A qualified signed release needs
 * PnP/power/storage verifier, cancellation, backing-device and crash testing.
 * SSD payloads are in Backing; RAM retains only the bounded index and one I/O block.
 */
#include <ntifs.h>
#include <ntddstor.h>
#include <ntdddisk.h>
#include <ntddvol.h>
#include <wdmsec.h>
#include "protocol.h"
#include "cache_state.h"
#ifndef SHDD_ENABLE_EXPERIMENTAL_ATTACH
#define SHDD_ENABLE_EXPERIMENTAL_ATTACH 0
#endif
#define SHDD_TAG 'dHSS'
typedef struct _ENTRY { ULONGLONG Block, Generation, Checksum; } ENTRY;
typedef struct _EXT {
    BOOLEAN Control;
    volatile LONG Stopping, Paused;
    PDEVICE_OBJECT Lower;
    PFILE_OBJECT SourceFile;
    HANDLE Backing;
    EX_RUNDOWN_REF Rundown;
    KMUTEX CacheMutex;
    KSPIN_LOCK QueueLock;
    LIST_ENTRY ReadQueue;
    KEVENT QueueReady;
    HANDLE ReaderThread;
    volatile LONG ThreadStopping;
    SHDD_EPOCH Epoch;
    volatile LONG PendingReads;
    ULONGLONG CapacityBytes, SourceLength;
    ULONG SlotCount;
    ENTRY *Entries;
    volatile LONG64 Hits,Misses,SourceReadBytes,SsdReadBytes,FillBytes,Evictions,Errors,ReadBypasses;
} EXT;
typedef struct _READ_WORK { LIST_ENTRY Link; PIRP Irp; } READ_WORK;
static PDEVICE_OBJECT gControl=NULL,gFilter=NULL;
static KMUTEX gConfiguration;
static UNICODE_STRING gLink=RTL_CONSTANT_STRING(L"\\DosDevices\\SuperHDDCacheDriver");
static const GUID gDeviceClass={0x47709d5d,0xefce,0x443e,{0x8c,0x56,0xd4,0xa1,0x71,0x88,0x41,0x7a}};
DRIVER_INITIALIZE DriverEntry;
DRIVER_UNLOAD ShddUnload;
DRIVER_DISPATCH ShddDispatch;
static IO_COMPLETION_ROUTINE ReleaseCompletion;
static KSTART_ROUTINE ReaderThread;
static NTSTATUS Complete(PIRP irp,NTSTATUS status,ULONG_PTR bytes){irp->IoStatus.Status=status;irp->IoStatus.Information=bytes;IoCompleteRequest(irp,IO_NO_INCREMENT);return status;}
static ULONGLONG Hash(const UCHAR *bytes,ULONG count){ULONGLONG h=0xcbf29ce484222325ULL;ULONG i;for(i=0;i<count;i++){h^=bytes[i];h*=0x100000001b3ULL;}return h;}
static VOID CloseBacking(HANDLE handle){
    FILE_DISPOSITION_INFORMATION disposition;IO_STATUS_BLOCK iosb;
    if(!handle)return;
    disposition.DeleteFile=TRUE;
    /* Only handles created with FILE_CREATE by this attachment reach this helper. */
    ZwSetInformationFile(handle,&iosb,&disposition,sizeof(disposition),FileDispositionInformation);
    ZwClose(handle);
}
static BOOLEAN Mutates(UCHAR major){
    return major==IRP_MJ_WRITE||major==IRP_MJ_DEVICE_CONTROL
        ||major==IRP_MJ_INTERNAL_DEVICE_CONTROL||major==IRP_MJ_FILE_SYSTEM_CONTROL
        ||major==IRP_MJ_FLUSH_BUFFERS;
}
static NTSTATUS ReleaseCompletion(PDEVICE_OBJECT device,PIRP irp,PVOID context){
    EXT *ext=(EXT*)context;UNREFERENCED_PARAMETER(device);
    if(!ext){if(irp->PendingReturned)IoMarkIrpPending(irp);return STATUS_CONTINUE_COMPLETION;}
    if(Mutates(IoGetCurrentIrpStackLocation(irp)->MajorFunction))ShddMutationEnd(&ext->Epoch);
    if(irp->PendingReturned)IoMarkIrpPending(irp);
    ExReleaseRundownProtection(&ext->Rundown);return STATUS_CONTINUE_COMPLETION;
}
static NTSTATUS Forward(EXT *ext,PIRP irp){
    if(Mutates(IoGetCurrentIrpStackLocation(irp)->MajorFunction))ShddMutationBegin(&ext->Epoch);
    IoCopyCurrentIrpStackLocationToNext(irp);IoSetCompletionRoutine(irp,ReleaseCompletion,ext,TRUE,TRUE,TRUE);
    return IoCallDriver(ext->Lower,irp);
}
static NTSTATUS SourceRead(EXT *ext,PVOID buffer,ULONG count,LONGLONG offset,PULONG_PTR bytes){
    KEVENT event;IO_STATUS_BLOCK iosb;LARGE_INTEGER position;PIRP request;NTSTATUS status;
    RtlZeroMemory(&iosb,sizeof(iosb));*bytes=0;
    KeInitializeEvent(&event,NotificationEvent,FALSE);position.QuadPart=offset;
    request=IoBuildSynchronousFsdRequest(IRP_MJ_READ,ext->Lower,buffer,count,&position,&event,&iosb);
    if(!request)return STATUS_INSUFFICIENT_RESOURCES;
    request->Flags|=IRP_NOCACHE;
    status=IoCallDriver(ext->Lower,request);if(status==STATUS_PENDING){KeWaitForSingleObject(&event,Executive,KernelMode,FALSE,NULL);status=iosb.Status;}
    *bytes=NT_SUCCESS(status)?iosb.Information:0;return status;
}
static VOID ReadWorker(PDEVICE_OBJECT device,PVOID context){
    READ_WORK *work=(READ_WORK*)context;EXT *ext=(EXT*)device->DeviceExtension;PIRP irp=work->Irp;
    PIO_STACK_LOCATION stack=IoGetCurrentIrpStackLocation(irp);ULONG length=stack->Parameters.Read.Length,done=0;LONGLONG start=stack->Parameters.Read.ByteOffset.QuadPart;
    UCHAR *output=(UCHAR*)MmGetSystemAddressForMdlSafe(irp->MdlAddress,NormalPagePriority|MdlMappingNoExecute);
    PVOID allocation=NULL;UCHAR *scratch=NULL;
    NTSTATUS status=STATUS_SUCCESS;
    KeWaitForSingleObject(&ext->CacheMutex,Executive,KernelMode,FALSE,NULL);
    allocation=ExAllocatePool2(POOL_FLAG_NON_PAGED,SHDD_BLOCK_BYTES+4095,SHDD_TAG);
    if(!output||!allocation){
        /* Cache resource pressure must not turn a valid source read into an error. */
        if(allocation)ExFreePoolWithTag(allocation,SHDD_TAG);
        KeReleaseMutex(&ext->CacheMutex,FALSE);
        ExFreePoolWithTag(work,SHDD_TAG);
        InterlockedDecrement(&ext->PendingReads);InterlockedIncrement64(&ext->ReadBypasses);
        Forward(ext,irp);return;
    }
    scratch=(UCHAR*)(((ULONG_PTR)allocation+4095)&~(ULONG_PTR)4095);
    while(done<length){
        ULONGLONG position=(ULONGLONG)start+done,block=position/SHDD_BLOCK_BYTES;
        ULONG blockOffset=(ULONG)(position%SHDD_BLOCK_BYTES),count=ShddSliceBytes(position,length-done);
        ULONG slot=(ULONG)(block%ext->SlotCount);ENTRY *entry=&ext->Entries[slot];
        LONG64 generation=ShddCaptureEpoch(&ext->Epoch);LARGE_INTEGER cacheOffset;IO_STATUS_BLOCK iosb;BOOLEAN hit=FALSE;ULONG_PTR readBytes=0;
        cacheOffset.QuadPart=(LONGLONG)slot*SHDD_BLOCK_BYTES;
        if(irp->Cancel){status=STATUS_CANCELLED;break;}
        if(!ShddAtomicRead(&ext->Paused)&&generation&&entry->Generation==(ULONGLONG)generation&&entry->Block==block){
            status=ZwReadFile(ext->Backing,NULL,NULL,NULL,&iosb,scratch,SHDD_BLOCK_BYTES,&cacheOffset,NULL);
            if(NT_SUCCESS(status)&&iosb.Information==SHDD_BLOCK_BYTES&&Hash(scratch,SHDD_BLOCK_BYTES)==entry->Checksum&&ShddEpochValid(&ext->Epoch,generation)){
                hit=TRUE;InterlockedIncrement64(&ext->Hits);InterlockedAdd64(&ext->SsdReadBytes,SHDD_BLOCK_BYTES);
            }else{entry->Generation=0;InterlockedIncrement64(&ext->Errors);}
        }
        if(!hit){
            InterlockedIncrement64(&ext->Misses);status=SourceRead(ext,scratch,SHDD_BLOCK_BYTES,(LONGLONG)(block*SHDD_BLOCK_BYTES),&readBytes);
            if(!NT_SUCCESS(status)||readBytes!=SHDD_BLOCK_BYTES){
                /* Failed speculative read-ahead must not fail a smaller valid
                   original read because an unrelated sector in this block failed. */
                entry->Generation=0;InterlockedIncrement64(&ext->Errors);
                if(irp->Cancel){status=STATUS_CANCELLED;break;}
                status=SourceRead(ext,scratch,count,(LONGLONG)position,&readBytes);
                if(!NT_SUCCESS(status))break;
                if(readBytes>count){status=STATUS_DEVICE_DATA_ERROR;break;}
                RtlCopyMemory(output+done,scratch,readBytes);done+=(ULONG)readBytes;
                InterlockedAdd64(&ext->SourceReadBytes,(LONG64)readBytes);
                if(readBytes<count)break;
                continue;
            }
            InterlockedAdd64(&ext->SourceReadBytes,(LONG64)readBytes);
            if(!ShddAtomicRead(&ext->Paused)&&ShddEpochValid(&ext->Epoch,generation)){
                if(entry->Generation==(ULONGLONG)generation&&entry->Block!=block)InterlockedIncrement64(&ext->Evictions);
                entry->Generation=0;
                status=ZwWriteFile(ext->Backing,NULL,NULL,NULL,&iosb,scratch,SHDD_BLOCK_BYTES,&cacheOffset,NULL);
                if(NT_SUCCESS(status)&&iosb.Information==SHDD_BLOCK_BYTES&&ShddEpochValid(&ext->Epoch,generation)){
                    entry->Block=block;entry->Checksum=Hash(scratch,SHDD_BLOCK_BYTES);entry->Generation=(ULONGLONG)generation;InterlockedAdd64(&ext->FillBytes,SHDD_BLOCK_BYTES);
                }else{InterlockedIncrement64(&ext->Errors);}
            }
        }
        RtlCopyMemory(output+done,scratch+blockOffset,count);done+=count;status=STATUS_SUCCESS;
    }
    KeReleaseMutex(&ext->CacheMutex,FALSE);
    if(allocation)ExFreePoolWithTag(allocation,SHDD_TAG);ExFreePoolWithTag(work,SHDD_TAG);
    InterlockedDecrement(&ext->PendingReads);Complete(irp,status,done);ExReleaseRundownProtection(&ext->Rundown);
}
/* A dedicated thread avoids exhausting the shared system work-item pool if a
   disposable backing device stalls. This is still subject to lab cancellation tests. */
static VOID ReaderThread(PVOID context){
    PDEVICE_OBJECT device=(PDEVICE_OBJECT)context;EXT *ext=(EXT*)device->DeviceExtension;
    for(;;){
        KIRQL irql;READ_WORK *work=NULL;BOOLEAN stop;
        KeAcquireSpinLock(&ext->QueueLock,&irql);
        if(!IsListEmpty(&ext->ReadQueue))work=CONTAINING_RECORD(RemoveHeadList(&ext->ReadQueue),READ_WORK,Link);
        else KeClearEvent(&ext->QueueReady);
        stop=ShddAtomicRead(&ext->ThreadStopping)!=0;
        KeReleaseSpinLock(&ext->QueueLock,irql);
        if(work){ReadWorker(device,work);continue;}
        if(stop)break;
        KeWaitForSingleObject(&ext->QueueReady,Executive,KernelMode,FALSE,NULL);
    }
    PsTerminateSystemThread(STATUS_SUCCESS);
}
static VOID StopReader(EXT *ext){
    if(!ext->ReaderThread)return;
    InterlockedExchange(&ext->ThreadStopping,1);KeSetEvent(&ext->QueueReady,IO_NO_INCREMENT,FALSE);
    ZwWaitForSingleObject(ext->ReaderThread,FALSE,NULL);ZwClose(ext->ReaderThread);ext->ReaderThread=NULL;
}
static VOID Detach(VOID){
    PDEVICE_OBJECT filter=gFilter;EXT *ext;if(!filter)return;gFilter=NULL;ext=(EXT*)filter->DeviceExtension;
    InterlockedExchange(&ext->Stopping,1);ShddDisable(&ext->Epoch);
    /* Control detach is lab-only and requires a quiesced source. Drain every
       forwarded/queued request before releasing the lower-device attachment. */
    ExWaitForRundownProtectionRelease(&ext->Rundown);StopReader(ext);IoDetachDevice(ext->Lower);
    CloseBacking(ext->Backing);ObDereferenceObject(ext->SourceFile);ExFreePoolWithTag(ext->Entries,SHDD_TAG);IoDeleteDevice(filter);
}
#if SHDD_ENABLE_EXPERIMENTAL_ATTACH
static NTSTATUS QueryDevice(PDEVICE_OBJECT device,ULONG code,PVOID input,ULONG inputBytes,PVOID output,ULONG outputBytes,PULONG_PTR returned){
    KEVENT event;IO_STATUS_BLOCK iosb;PIRP irp;NTSTATUS status;
    RtlZeroMemory(&iosb,sizeof(iosb));*returned=0;KeInitializeEvent(&event,NotificationEvent,FALSE);
    irp=IoBuildDeviceIoControlRequest(code,device,input,inputBytes,output,outputBytes,FALSE,&event,&iosb);
    if(!irp)return STATUS_INSUFFICIENT_RESOURCES;
    status=IoCallDriver(device,irp);
    if(status==STATUS_PENDING){KeWaitForSingleObject(&event,Executive,KernelMode,FALSE,NULL);status=iosb.Status;}
    *returned=iosb.Information;return status;
}
static NTSTATUS LabDisk(PDEVICE_OBJECT device,PULONG diskNumber){
    STORAGE_PROPERTY_QUERY query;UCHAR buffer[1024];STORAGE_DEVICE_DESCRIPTOR *descriptor=(STORAGE_DEVICE_DESCRIPTOR*)buffer;
    VOLUME_DISK_EXTENTS extents;ULONG_PTR returned;NTSTATUS status;
    RtlZeroMemory(&query,sizeof(query));RtlZeroMemory(buffer,sizeof(buffer));
    query.PropertyId=StorageDeviceProperty;query.QueryType=PropertyStandardQuery;
    status=QueryDevice(device,IOCTL_STORAGE_QUERY_PROPERTY,&query,sizeof(query),buffer,sizeof(buffer),&returned);
    if(!NT_SUCCESS(status)||returned<sizeof(*descriptor)||descriptor->RemovableMedia
        ||descriptor->BusType!=BusTypeFileBackedVirtual)return STATUS_NOT_SUPPORTED;
    RtlZeroMemory(&extents,sizeof(extents));
    status=QueryDevice(device,IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,NULL,0,&extents,sizeof(extents),&returned);
    if(!NT_SUCCESS(status)||returned<sizeof(extents)||extents.NumberOfDiskExtents!=1
        ||extents.Extents[0].StartingOffset.QuadPart<0||extents.Extents[0].ExtentLength.QuadPart<=0)return STATUS_NOT_SUPPORTED;
    *diskNumber=extents.Extents[0].DiskNumber;return STATUS_SUCCESS;
}
static NTSTATUS BackingParent(PCWSTR path,ULONGLONG capacity,HANDLE *directory,PULONG diskNumber){
    WCHAR parent[520];ULONG count=0,split=0,index;UNICODE_STRING name;OBJECT_ATTRIBUTES attributes;
    IO_STATUS_BLOCK iosb;PFILE_OBJECT file=NULL;PDEVICE_OBJECT volume=NULL;NTSTATUS status;
    UCHAR fsBuffer[sizeof(FILE_FS_ATTRIBUTE_INFORMATION)+32];FILE_FS_ATTRIBUTE_INFORMATION *fs=(FILE_FS_ATTRIBUTE_INFORMATION*)fsBuffer;
    FILE_FS_SIZE_INFORMATION size;ULONGLONG unit,total,available,reserve;
    *directory=NULL;
    while(path[count]){if(path[count]==L'\\')split=count;count++;}
    if(!split||!path[split+1]||path[split+1]==L'.')return STATUS_OBJECT_NAME_INVALID;
    for(index=split+1;index<count;index++)if(path[index]<L' '||path[index]==L':')return STATUS_OBJECT_NAME_INVALID;
    RtlCopyMemory(parent,path,(split+1)*sizeof(WCHAR));parent[split+1]=0;RtlInitUnicodeString(&name,parent);
    InitializeObjectAttributes(&attributes,&name,OBJ_CASE_INSENSITIVE|OBJ_KERNEL_HANDLE|OBJ_DONT_REPARSE,NULL,NULL);
    status=ZwOpenFile(directory,FILE_READ_ATTRIBUTES|SYNCHRONIZE,&attributes,&iosb,FILE_SHARE_READ|FILE_SHARE_WRITE,
        FILE_DIRECTORY_FILE|FILE_SYNCHRONOUS_IO_NONALERT|FILE_OPEN_REPARSE_POINT);
    if(!NT_SUCCESS(status))return status;
    RtlZeroMemory(fsBuffer,sizeof(fsBuffer));
    status=ZwQueryVolumeInformationFile(*directory,&iosb,fsBuffer,sizeof(fsBuffer),FileFsAttributeInformation);
    if(!NT_SUCCESS(status)||fs->FileSystemNameLength!=8||RtlCompareMemory(fs->FileSystemName,L"NTFS",8)!=8){status=STATUS_NOT_SUPPORTED;goto finished;}
    status=ObReferenceObjectByHandle(*directory,FILE_READ_ATTRIBUTES,*IoFileObjectType,KernelMode,(PVOID*)&file,NULL);
    if(!NT_SUCCESS(status))goto finished;
    if(file->DeviceObject->DeviceType!=FILE_DEVICE_DISK_FILE_SYSTEM){status=STATUS_NOT_SUPPORTED;goto finished;}
    status=IoGetDiskDeviceObject(file->DeviceObject,&volume);
    if(!NT_SUCCESS(status))goto finished;
    if(volume->AlignmentRequirement>4095){status=STATUS_NOT_SUPPORTED;goto finished;}
    status=LabDisk(volume,diskNumber);if(!NT_SUCCESS(status))goto finished;
    status=ZwQueryVolumeInformationFile(*directory,&iosb,&size,sizeof(size),FileFsSizeInformation);
    if(!NT_SUCCESS(status))goto finished;
    unit=(ULONGLONG)size.SectorsPerAllocationUnit*size.BytesPerSector;
    if(!unit||size.BytesPerSector>4096||4096%size.BytesPerSector
        ||size.TotalAllocationUnits.QuadPart<=0||size.AvailableAllocationUnits.QuadPart<0
        ||size.AvailableAllocationUnits.QuadPart>size.TotalAllocationUnits.QuadPart
        ||(ULONGLONG)size.TotalAllocationUnits.QuadPart>MAXULONGLONG/unit){status=STATUS_INVALID_PARAMETER;goto finished;}
    total=(ULONGLONG)size.TotalAllocationUnits.QuadPart*unit;available=(ULONGLONG)size.AvailableAllocationUnits.QuadPart*unit;
    reserve=max(total/10,64ULL*1024*1024);
    if(available<reserve||capacity>available-reserve)status=STATUS_DISK_FULL;
finished:
    if(volume)ObDereferenceObject(volume);if(file)ObDereferenceObject(file);
    if(!NT_SUCCESS(status)){ZwClose(*directory);*directory=NULL;}
    return status;
}
static NTSTATUS Attach(PDRIVER_OBJECT driver,SHDD_ATTACH *request){
    UNICODE_STRING source,backing;PFILE_OBJECT sourceFile=NULL;PDEVICE_OBJECT sourceDevice=NULL,filter=NULL;
    OBJECT_ATTRIBUTES attributes;IO_STATUS_BLOCK iosb;HANDLE handle=NULL,parent=NULL;LARGE_INTEGER allocation;
    FILE_END_OF_FILE_INFORMATION eof;FILE_ATTRIBUTE_TAG_INFORMATION tags;GET_LENGTH_INFORMATION length;
    NTSTATUS status;EXT *ext;ULONG slots,sourceDisk,backingDisk,index=0,split=0;ULONG_PTR returned;
    if(gFilter||request->Version!=SHDD_PROTOCOL_VERSION||request->DisposableVolumeAcknowledged!=0x54455354)return STATUS_INVALID_PARAMETER;
    if(request->SourceDevice[259]!=0||request->BackingFile[519]!=0||!ShddCapacityValid(request->CapacityBytes))return STATUS_INVALID_PARAMETER;
    RtlInitUnicodeString(&source,request->SourceDevice);
    /* Lab source must be unmounted. Opening with READ_DATA could mount an FSD
       and accidentally intercept file-relative rather than volume-relative I/O. */
    status=IoGetDeviceObjectPointer(&source,FILE_READ_ATTRIBUTES,&sourceFile,&sourceDevice);if(!NT_SUCCESS(status))return status;
    if(sourceFile->DeviceObject->DeviceType!=FILE_DEVICE_DISK||sourceDevice->DeviceType!=FILE_DEVICE_DISK
        ||(sourceFile->Vpb&&(sourceFile->Vpb->Flags&VPB_MOUNTED))
        ||!(sourceDevice->Flags&DO_DIRECT_IO)||sourceDevice->AlignmentRequirement>4095){status=STATUS_NOT_SUPPORTED;goto failed;}
    status=LabDisk(sourceDevice,&sourceDisk);if(!NT_SUCCESS(status))goto failed;
    status=QueryDevice(sourceDevice,IOCTL_DISK_GET_LENGTH_INFO,NULL,0,&length,sizeof(length),&returned);
    if(!NT_SUCCESS(status)||returned<sizeof(length)||length.Length.QuadPart<SHDD_BLOCK_BYTES){status=STATUS_NOT_SUPPORTED;goto failed;}
    status=BackingParent(request->BackingFile,request->CapacityBytes,&parent,&backingDisk);if(!NT_SUCCESS(status))goto failed;
    if(sourceDisk==backingDisk){status=STATUS_INVALID_PARAMETER;goto failed;}
    while(request->BackingFile[index]){if(request->BackingFile[index]==L'\\')split=index;index++;}
    RtlInitUnicodeString(&backing,&request->BackingFile[split+1]);
    InitializeObjectAttributes(&attributes,&backing,OBJ_CASE_INSENSITIVE|OBJ_KERNEL_HANDLE|OBJ_DONT_REPARSE,parent,NULL);allocation.QuadPart=request->CapacityBytes;
    status=ZwCreateFile(&handle,SHDD_BACKING_ACCESS,&attributes,&iosb,&allocation,FILE_ATTRIBUTE_NORMAL,0,FILE_CREATE,FILE_NON_DIRECTORY_FILE|FILE_SYNCHRONOUS_IO_NONALERT|FILE_NO_INTERMEDIATE_BUFFERING|FILE_OPEN_REPARSE_POINT,NULL,0);
    if(!NT_SUCCESS(status))goto failed;
    status=ZwQueryInformationFile(handle,&iosb,&tags,sizeof(tags),FileAttributeTagInformation);
    if(!NT_SUCCESS(status)||(tags.FileAttributes&(FILE_ATTRIBUTE_COMPRESSED|FILE_ATTRIBUTE_ENCRYPTED|FILE_ATTRIBUTE_SPARSE_FILE|FILE_ATTRIBUTE_REPARSE_POINT))){status=STATUS_NOT_SUPPORTED;goto failed;}
    eof.EndOfFile.QuadPart=request->CapacityBytes;
    status=ZwSetInformationFile(handle,&iosb,&eof,sizeof(eof),FileEndOfFileInformation);if(!NT_SUCCESS(status))goto failed;
    status=IoCreateDevice(driver,sizeof(EXT),NULL,sourceDevice->DeviceType,0,FALSE,&filter);if(!NT_SUCCESS(status))goto failed;
    ext=(EXT*)filter->DeviceExtension;RtlZeroMemory(ext,sizeof(*ext));ext->Backing=handle;ext->SourceFile=sourceFile;ext->Epoch.Generation=1;ext->CapacityBytes=request->CapacityBytes;ext->SourceLength=length.Length.QuadPart;
    slots=(ULONG)(request->CapacityBytes/SHDD_BLOCK_BYTES);ext->SlotCount=slots;ext->Entries=ExAllocatePool2(POOL_FLAG_NON_PAGED,sizeof(ENTRY)*slots,SHDD_TAG);
    if(!ext->Entries){status=STATUS_INSUFFICIENT_RESOURCES;goto failed;}
    RtlZeroMemory(ext->Entries,sizeof(ENTRY)*slots);
    KeInitializeMutex(&ext->CacheMutex,0);ExInitializeRundownProtection(&ext->Rundown);
    KeInitializeSpinLock(&ext->QueueLock);InitializeListHead(&ext->ReadQueue);KeInitializeEvent(&ext->QueueReady,NotificationEvent,FALSE);
    InitializeObjectAttributes(&attributes,NULL,OBJ_KERNEL_HANDLE,NULL,NULL);
    status=PsCreateSystemThread(&ext->ReaderThread,THREAD_ALL_ACCESS,&attributes,NULL,NULL,ReaderThread,filter);
    if(!NT_SUCCESS(status))goto failed;
    filter->Flags|=sourceDevice->Flags&(DO_DIRECT_IO|DO_BUFFERED_IO|DO_POWER_PAGABLE);filter->AlignmentRequirement=sourceDevice->AlignmentRequirement;
    status=IoAttachDeviceToDeviceStackSafe(filter,sourceDevice,&ext->Lower);
    if(!NT_SUCCESS(status))goto failed;
    gFilter=filter;filter->Flags&=~DO_DEVICE_INITIALIZING;ZwClose(parent);return STATUS_SUCCESS;
failed:
    if(filter){ext=(EXT*)filter->DeviceExtension;StopReader(ext);if(ext->Entries)ExFreePoolWithTag(ext->Entries,SHDD_TAG);IoDeleteDevice(filter);}
    CloseBacking(handle);if(parent)ZwClose(parent);if(sourceFile)ObDereferenceObject(sourceFile);return status;
}
#endif
static NTSTATUS Control(PDEVICE_OBJECT device,PIRP irp){
    PIO_STACK_LOCATION stack=IoGetCurrentIrpStackLocation(irp);ULONG code=stack->Parameters.DeviceIoControl.IoControlCode;NTSTATUS status=STATUS_INVALID_DEVICE_REQUEST;ULONG_PTR size=0;
    if(stack->MajorFunction==IRP_MJ_CREATE||stack->MajorFunction==IRP_MJ_CLOSE||stack->MajorFunction==IRP_MJ_CLEANUP)return Complete(irp,STATUS_SUCCESS,0);
    if(stack->MajorFunction!=IRP_MJ_DEVICE_CONTROL)return Complete(irp,status,0);
    KeWaitForSingleObject(&gConfiguration,Executive,KernelMode,FALSE,NULL);
    if(code==SHDD_IOCTL_STATUS&&stack->Parameters.DeviceIoControl.OutputBufferLength>=sizeof(SHDD_STATUS)){
        SHDD_STATUS *out=(SHDD_STATUS*)irp->AssociatedIrp.SystemBuffer;RtlZeroMemory(out,sizeof(*out));out->Version=SHDD_PROTOCOL_VERSION;out->ExperimentalAttachEnabled=SHDD_ENABLE_EXPERIMENTAL_ATTACH;
        out->QueueLimit=SHDD_MAX_PENDING_READS;out->MaxReadBytes=SHDD_MAX_READ_BYTES;
        if(gFilter){EXT *e=(EXT*)gFilter->DeviceExtension;out->Attached=1;out->Paused=ShddAtomicRead(&e->Paused);out->Hits=e->Hits;out->Misses=e->Misses;out->SourceReadBytes=e->SourceReadBytes;out->SsdReadBytes=e->SsdReadBytes;out->FillBytes=e->FillBytes;out->Evictions=e->Evictions;out->Errors=e->Errors;out->Generation=ShddGeneration(&e->Epoch);out->CapacityBytes=e->CapacityBytes;out->SourceLength=e->SourceLength;out->ActiveMutations=ShddAtomicRead(&e->Epoch.ActiveMutations);out->PendingReads=ShddAtomicRead(&e->PendingReads);out->CacheDisabled=ShddAtomicRead(&e->Epoch.Disabled);out->ReadBypasses=e->ReadBypasses;}size=sizeof(*out);status=STATUS_SUCCESS;
    }else if(code==SHDD_IOCTL_ATTACH){
#if SHDD_ENABLE_EXPERIMENTAL_ATTACH
        if(stack->Parameters.DeviceIoControl.InputBufferLength==sizeof(SHDD_ATTACH))status=Attach(device->DriverObject,(SHDD_ATTACH*)irp->AssociatedIrp.SystemBuffer);
#else
        UNREFERENCED_PARAMETER(device);status=STATUS_NOT_SUPPORTED;
#endif
    }else if(code==SHDD_IOCTL_DETACH){Detach();status=STATUS_SUCCESS;
    }else if(code==SHDD_IOCTL_CLEAR&&gFilter){ShddInvalidate(&((EXT*)gFilter->DeviceExtension)->Epoch);status=STATUS_SUCCESS;
    }else if(code==SHDD_IOCTL_PAUSE&&gFilter&&stack->Parameters.DeviceIoControl.InputBufferLength==sizeof(ULONG)){EXT *e=(EXT*)gFilter->DeviceExtension;InterlockedExchange(&e->Paused,*(ULONG*)irp->AssociatedIrp.SystemBuffer?1:0);ShddInvalidate(&e->Epoch);status=STATUS_SUCCESS;}
    KeReleaseMutex(&gConfiguration,FALSE);return Complete(irp,status,size);
}
NTSTATUS ShddDispatch(PDEVICE_OBJECT device,PIRP irp){
    EXT *ext=(EXT*)device->DeviceExtension;PIO_STACK_LOCATION stack=IoGetCurrentIrpStackLocation(irp);
    if(ext->Control){if(KeGetCurrentIrql()!=PASSIVE_LEVEL)return Complete(irp,STATUS_INVALID_DEVICE_STATE,0);return Control(device,irp);}
    if(!ExAcquireRundownProtection(&ext->Rundown))return Complete(irp,STATUS_DELETE_PENDING,0);
    if(stack->MajorFunction==IRP_MJ_PNP||stack->MajorFunction==IRP_MJ_POWER||stack->MajorFunction==IRP_MJ_SHUTDOWN)ShddDisable(&ext->Epoch);
    if(stack->MajorFunction==IRP_MJ_READ&&!ShddAtomicRead(&ext->Stopping)&&!ShddAtomicRead(&ext->Paused)&&!ShddAtomicRead(&ext->Epoch.Disabled)&&irp->MdlAddress&&!irp->MdlAddress->Next
        &&ShddEligibleRead(stack->Parameters.Read.ByteOffset.QuadPart,stack->Parameters.Read.Length,ext->SourceLength,MmGetMdlByteCount(irp->MdlAddress),(irp->Flags&(IRP_PAGING_IO|IRP_SYNCHRONOUS_PAGING_IO))!=0)
        &&ShddReserveRead(&ext->PendingReads)){
        READ_WORK *work=ExAllocatePool2(POOL_FLAG_NON_PAGED,sizeof(READ_WORK),SHDD_TAG);
        if(work){KIRQL irql;work->Irp=irp;IoMarkIrpPending(irp);
            KeAcquireSpinLock(&ext->QueueLock,&irql);InsertTailList(&ext->ReadQueue,&work->Link);
            KeSetEvent(&ext->QueueReady,IO_NO_INCREMENT,FALSE);KeReleaseSpinLock(&ext->QueueLock,irql);return STATUS_PENDING;}
        InterlockedDecrement(&ext->PendingReads);
    }
    if(stack->MajorFunction==IRP_MJ_READ)InterlockedIncrement64(&ext->ReadBypasses);
    return Forward(ext,irp);
}
VOID ShddUnload(PDRIVER_OBJECT driver){UNREFERENCED_PARAMETER(driver);KeWaitForSingleObject(&gConfiguration,Executive,KernelMode,FALSE,NULL);Detach();KeReleaseMutex(&gConfiguration,FALSE);IoDeleteSymbolicLink(&gLink);if(gControl)IoDeleteDevice(gControl);}
NTSTATUS DriverEntry(PDRIVER_OBJECT driver,PUNICODE_STRING registry){
    UNICODE_STRING name=RTL_CONSTANT_STRING(L"\\Device\\SuperHDDCacheDriver");UNICODE_STRING sddl=RTL_CONSTANT_STRING(L"D:P(A;;GA;;;SY)(A;;GA;;;BA)");NTSTATUS status;ULONG index;UNREFERENCED_PARAMETER(registry);
    KeInitializeMutex(&gConfiguration,0);for(index=0;index<=IRP_MJ_MAXIMUM_FUNCTION;index++)driver->MajorFunction[index]=ShddDispatch;driver->DriverUnload=ShddUnload;
    status=IoCreateDeviceSecure(driver,sizeof(EXT),&name,FILE_DEVICE_UNKNOWN,FILE_DEVICE_SECURE_OPEN,FALSE,&sddl,&gDeviceClass,&gControl);if(!NT_SUCCESS(status))return status;
    RtlZeroMemory(gControl->DeviceExtension,sizeof(EXT));((EXT*)gControl->DeviceExtension)->Control=TRUE;gControl->Flags|=DO_BUFFERED_IO;
    status=IoCreateSymbolicLink(&gLink,&name);if(!NT_SUCCESS(status)){IoDeleteDevice(gControl);gControl=NULL;return status;}gControl->Flags&=~DO_DEVICE_INITIALIZING;return STATUS_SUCCESS;
}
