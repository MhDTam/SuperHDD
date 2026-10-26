/* Query all facts independently in the kernel before opening writable backing. */
static NTSTATUS DirectFacts(PDEVICE_OBJECT device, SHDD_SOURCE_FACTS *facts) {
    STORAGE_PROPERTY_QUERY query; UCHAR bytes[1024]; ULONG_PTR returned; NTSTATUS status;
    STORAGE_DEVICE_DESCRIPTOR *d=(STORAGE_DEVICE_DESCRIPTOR*)bytes;
    DEVICE_SEEK_PENALTY_DESCRIPTOR seek; VOLUME_DISK_EXTENTS extents;
    ULONG i;
    RtlZeroMemory(&query,sizeof(query));RtlZeroMemory(bytes,sizeof(bytes));
    query.PropertyId=StorageDeviceProperty;
    status=Query(device,IOCTL_STORAGE_QUERY_PROPERTY,&query,sizeof(query),bytes,sizeof(bytes),&returned);
    if(!NT_SUCCESS(status)||returned<sizeof(*d)||d->Size<sizeof(*d)||d->Size>returned) return STATUS_NOT_SUPPORTED;
    facts->Bus=(ULONG)d->BusType;facts->Removable=d->RemovableMedia;
    facts->SystemRole=(device->Flags&(DO_SYSTEM_BOOT_PARTITION|DO_SYSTEM_SYSTEM_PARTITION))!=0;
    if(d->SerialNumberOffset>=sizeof(*d)&&d->SerialNumberOffset<d->Size) {
        for(i=d->SerialNumberOffset;i<d->Size;i++) {
            if(!bytes[i]) {facts->SerialPresent=i>d->SerialNumberOffset;break;}
            if(bytes[i]<32||bytes[i]>126) break;
        }
    }
    RtlZeroMemory(&extents,sizeof(extents));
    status=Query(device,IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,NULL,0,&extents,sizeof(extents),&returned);
    if(!NT_SUCCESS(status)||returned<sizeof(extents)||extents.NumberOfDiskExtents!=1
        ||extents.Extents[0].StartingOffset.QuadPart<=0||extents.Extents[0].ExtentLength.QuadPart<=0) return STATUS_NOT_SUPPORTED;
    facts->SingleExtent=1;facts->ActualDisk=extents.Extents[0].DiskNumber;
    facts->Offset=extents.Extents[0].StartingOffset.QuadPart;facts->Length=extents.Extents[0].ExtentLength.QuadPart;
    RtlZeroMemory(&seek,sizeof(seek));query.PropertyId=StorageDeviceSeekPenaltyProperty;
    status=Query(device,IOCTL_STORAGE_QUERY_PROPERTY,&query,sizeof(query),&seek,sizeof(seek),&returned);
    if(NT_SUCCESS(status)&&returned>=sizeof(seek)&&seek.Size>=sizeof(seek)&&seek.Version>=sizeof(seek)) {
        facts->SeekKnown=1;facts->IncursSeek=seek.IncursSeekPenalty;
    }
    return STATUS_SUCCESS;
}
static NTSTATUS DirectSource(VOLUME *volume, SHDD_ATTACH *request, PULONG number) {
    SHDD_SOURCE_FACTS source, parent; NTSTATUS status; HANDLE handle=NULL;
    UNICODE_STRING name;OBJECT_ATTRIBUTES attributes;IO_STATUS_BLOCK iosb;
    PFILE_OBJECT file=NULL;PDEVICE_OBJECT storage=NULL;
    RtlZeroMemory(&source,sizeof(source));RtlZeroMemory(&parent,sizeof(parent));
    source.ExpectedDisk=request->SourceDisk;source.ExpectedOffset=request->SourceOffset;source.ExpectedLength=request->SourceBytes;
    status=DirectFacts(volume->Lower,&source);if(!NT_SUCCESS(status))return status;
    source.SpecialUsage=ShddAtomicRead(&volume->SpecialUsage)!=0;
    source.SystemRole|=(volume->Pdo->Flags&(DO_SYSTEM_BOOT_PARTITION|DO_SYSTEM_SYSTEM_PARTITION))!=0;
    if(!ShddDirectSourceAllowed(&source,request->DisposableVolumeAcknowledged,SHDD_DIRECT_VM_BUILD))return STATUS_NOT_SUPPORTED;
    if(!ShddWirePathValid(request->CacheParentDevice,260,FALSE))return STATUS_INVALID_PARAMETER;
    RtlInitUnicodeString(&name,request->CacheParentDevice);
    InitializeObjectAttributes(&attributes,&name,OBJ_CASE_INSENSITIVE|OBJ_KERNEL_HANDLE|OBJ_DONT_REPARSE,NULL,NULL);
    status=ZwOpenFile(&handle,FILE_READ_DATA|FILE_READ_ATTRIBUTES|SYNCHRONIZE,&attributes,&iosb,
        FILE_SHARE_READ|FILE_SHARE_WRITE,FILE_SYNCHRONOUS_IO_NONALERT);
    if(!NT_SUCCESS(status))return status;
    status=Ntfs(handle);if(!NT_SUCCESS(status))goto done;
    status=ObReferenceObjectByHandle(handle,FILE_READ_DATA,*IoFileObjectType,KernelMode,(PVOID*)&file,NULL);
    if(!NT_SUCCESS(status))goto done;
    if(!(file->Flags&FO_VOLUME_OPEN)){status=STATUS_NOT_SUPPORTED;goto done;}
    status=StorageDevice(file,&storage);if(!NT_SUCCESS(status))goto done;
    parent.ExpectedDisk=request->CacheParentDisk;
    status=DirectFacts(storage,&parent);if(!NT_SUCCESS(status))goto done;
    if(!ShddDirectParentAllowed(&parent,source.ActualDisk,request->DisposableVolumeAcknowledged,SHDD_DIRECT_VM_BUILD)) {
        status=STATUS_NOT_SUPPORTED;goto done;
    }
    *number=source.ActualDisk;
done:
    if(storage)ObDereferenceObject(storage);if(file)ObDereferenceObject(file);if(handle)ZwClose(handle);return status;
}
