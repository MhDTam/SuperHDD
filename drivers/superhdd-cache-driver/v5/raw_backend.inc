/* Only included by the v4 disposable-VHD lab build. Cache I/O bypasses the
 * filesystem, but the RAW volume stays locked by our private handle. No boot,
 * physical, formatted, or unmarked cache volume is accepted. */
static NTSTATUS RawIo(VOLUME *volume, PIRP original, UCHAR major, PVOID buffer, ULONG bytes, ULONGLONG offset) {
    ULONG_PTR returned; NTSTATUS status;
    if (!volume->RawDevice || !volume->StoreLocked
        || !ShddStoreRange(offset, bytes, volume->StoreHeader.TotalBytes)) return STATUS_INVALID_PARAMETER;
    status = Transfer(volume, original, volume->RawDevice, NULL, major, buffer, bytes, offset, &returned);
    return NT_SUCCESS(status) && returned != bytes ? STATUS_DEVICE_DATA_ERROR : status;
}
static NTSTATUS RawFsControl(VOLUME *volume, ULONG code) {
    IO_STATUS_BLOCK iosb; NTSTATUS status;
    status = ZwFsControlFile(volume->Backing, NULL, NULL, NULL, &iosb, code, NULL, 0, NULL, 0);
    return WaitMetadata(volume->Backing, status, &iosb);
}
static NTSTATUS RawZeroIndex(VOLUME *volume) {
    ULONGLONG offset; NTSTATUS status;
    RtlZeroMemory(volume->Metadata, SHDD_STORE_SECTOR);
    for (offset = SHDD_STORE_SECTOR; offset < volume->StoreHeader.DataOffset; offset += SHDD_STORE_SECTOR) {
        status = RawIo(volume, NULL, IRP_MJ_WRITE, volume->Metadata, SHDD_STORE_SECTOR, offset);
        if (!NT_SUCCESS(status)) return status;
    }
    return STATUS_SUCCESS;
}
static NTSTATUS RawOpen(VOLUME *volume, SHDD_ATTACH *request, HANDLE source, ULONG sourceDisk, PULONG backingDisk) {
    UNICODE_STRING name, rawName = RTL_CONSTANT_STRING(L"\\FileSystem\\RAW"); OBJECT_ATTRIBUTES attributes;
    IO_STATUS_BLOCK iosb; NTSTATUS status; PDEVICE_OBJECT storage = NULL;
    GET_LENGTH_INFORMATION length; ULONG_PTR returned; SHDD_STORE_HEADER expected, saved;
    UCHAR sourceInfo[sizeof(FILE_FS_VOLUME_INFORMATION) + 512];
    FILE_FS_VOLUME_INFORMATION *identity = (FILE_FS_VOLUME_INFORMATION*)sourceInfo;
    if (!ShddStoreLayout(request->CapacityBytes, &expected)) return STATUS_INVALID_PARAMETER;
    volume->MetadataAllocation = ExAllocatePool2(POOL_FLAG_NON_PAGED, SHDD_STORE_SECTOR + 4095, TAG);
    if (!volume->MetadataAllocation) return STATUS_INSUFFICIENT_RESOURCES;
    volume->Metadata = (UCHAR*)(((ULONG_PTR)volume->MetadataAllocation + 4095) & ~(ULONG_PTR)4095);
    ConfigStep(20);
    RtlInitUnicodeString(&name, request->BackingFile);
    InitializeObjectAttributes(&attributes, &name, OBJ_CASE_INSENSITIVE | OBJ_KERNEL_HANDLE | OBJ_DONT_REPARSE, NULL, NULL);
    status = ZwOpenFile(&volume->Backing, FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        &attributes, &iosb, FILE_SHARE_READ | FILE_SHARE_WRITE,
        FILE_SYNCHRONOUS_IO_NONALERT | FILE_NO_INTERMEDIATE_BUFFERING | FILE_NON_DIRECTORY_FILE);
    if (!NT_SUCCESS(status)) return status;
    ConfigStep(21);
    status = ObReferenceObjectByHandle(volume->Backing, FILE_READ_DATA | FILE_WRITE_DATA,
        *IoFileObjectType, KernelMode, (PVOID*)&volume->BackingFile, NULL);
    if (!NT_SUCCESS(status)) return status;
    ConfigStep(22);gDiagnostic.CacheFileFlags=volume->BackingFile->Flags;gDiagnostic.CacheDeviceType=volume->BackingFile->DeviceObject->DeviceType;
    if (!(volume->BackingFile->Flags & FO_VOLUME_OPEN)) return STATUS_NOT_SUPPORTED;
    ConfigStep(23);status = StorageDevice(volume->BackingFile, &storage);
    if (!NT_SUCCESS(status)) return status;
    volume->RawDevice = IoGetAttachedDeviceReference(storage); ObDereferenceObject(storage);
    if (!(volume->RawDevice->Flags & DO_DIRECT_IO) || volume->RawDevice->AlignmentRequirement > 4095) return STATUS_NOT_SUPPORTED;
    ConfigStep(24);status = LabDisk(volume->RawDevice, backingDisk);
    if (!NT_SUCCESS(status)) return status;
    if (*backingDisk == sourceDisk) return STATUS_INVALID_PARAMETER;
    ConfigStep(25);status = Query(volume->RawDevice, IOCTL_DISK_GET_LENGTH_INFO, NULL, 0, &length, sizeof(length), &returned);
    if (!NT_SUCCESS(status) || returned < sizeof(length) || length.Length.QuadPart < 0
        || (ULONGLONG)length.Length.QuadPart < request->CapacityBytes) return STATUS_DISK_FULL;
    RtlZeroMemory(sourceInfo, sizeof(sourceInfo));
    status = ZwQueryVolumeInformationFile(source, &iosb, sourceInfo, sizeof(sourceInfo), FileFsVolumeInformation);
    if (!NT_SUCCESS(status)) return status;
    ConfigStep(26);status = RawFsControl(volume, FSCTL_LOCK_VOLUME);
    if (!NT_SUCCESS(status)) return status;
    volume->StoreLocked = TRUE; volume->StoreHeader = expected;
    /* Volume FILE_OBJECT.DeviceObject is the storage device, including for
     * NTFS/RAW opens. Resolve the filesystem stack through the public API and
     * inspect its referenced base while our volume lock is held. */
    {
        PDEVICE_OBJECT filesystem=IoGetDeviceAttachmentBaseRef(IoGetRelatedDeviceObject(volume->BackingFile));
        BOOLEAN raw=filesystem->DeviceType==FILE_DEVICE_DISK_FILE_SYSTEM
            &&RtlEqualUnicodeString(&filesystem->DriverObject->DriverName,&rawName,TRUE);
        gDiagnostic.CacheDeviceType=filesystem->DeviceType;
        ObDereferenceObject(filesystem);if(!raw)return STATUS_NOT_SUPPORTED;
    }
    ConfigStep(27);status = RawIo(volume, NULL, IRP_MJ_READ, volume->Metadata, SHDD_STORE_SECTOR, 0);
    if (!NT_SUCCESS(status)) return status;
    RtlCopyMemory(&saved, volume->Metadata, sizeof(saved));
    ConfigStep(28);if (!ShddHeaderValid(&saved)) return STATUS_UNRECOGNIZED_VOLUME;
    if (saved.Flags == SHDD_STORE_BOOTSTRAP) {
        RtlCopyMemory(expected.InstanceId, saved.InstanceId, sizeof(expected.InstanceId));
        expected.SourceLength = volume->SourceLength; expected.SourceSerial = identity->VolumeSerialNumber;
        expected.Flags = SHDD_STORE_READY; expected.HeaderHash = ShddHeaderHash(&expected);
        volume->StoreHeader = expected;
        status = RawZeroIndex(volume); if (!NT_SUCCESS(status)) return status;
        RtlZeroMemory(volume->Metadata, SHDD_STORE_SECTOR); RtlCopyMemory(volume->Metadata, &expected, sizeof(expected));
        status = RawIo(volume, NULL, IRP_MJ_WRITE, volume->Metadata, SHDD_STORE_SECTOR, 0);
        if (!NT_SUCCESS(status)) return status;
    } else {
        if (saved.TotalBytes != expected.TotalBytes || saved.SlotCount != expected.SlotCount
            || saved.DataOffset != expected.DataOffset || saved.SourceLength != volume->SourceLength
            || saved.SourceSerial != identity->VolumeSerialNumber) return STATUS_INVALID_PARAMETER;
        volume->StoreHeader = saved;
    }
    /* Every imported record remains generation zero: first touch reads current
     * source bytes. Persisted metadata never authorizes a warm hit on its own. */
    return STATUS_SUCCESS;
}
static NTSTATUS RawLoad(VOLUME *volume) {
    ULONG slot; ULONGLONG page = MAXULONGLONG; NTSTATUS status;
    volume->ImportedRecords = volume->RevalidatedRecords = volume->ValidationReadBytes = volume->PagingHits = 0;
    for (slot = 0; slot < volume->SlotCount; slot++) {
        ULONGLONG offset = SHDD_STORE_SECTOR + (ULONGLONG)slot * sizeof(SHDD_STORE_RECORD);
        ULONGLONG current = offset & ~(ULONGLONG)(SHDD_STORE_SECTOR - 1); SHDD_STORE_RECORD record;
        if (page != current) {
            status = RawIo(volume, NULL, IRP_MJ_READ, volume->Metadata, SHDD_STORE_SECTOR, current);
            if (!NT_SUCCESS(status)) return status; page = current;
        }
        RtlCopyMemory(&record, volume->Metadata + (ULONG)(offset - page), sizeof(record));
        if (ShddRecordValid(&record, slot, volume->SlotCount, volume->SourceLength)) {
            SLOT *entry = &volume->Slots[slot]; entry->Block = record.Block; entry->RecordValid = TRUE;
            RtlCopyMemory(entry->Digest, record.Digest, sizeof(entry->Digest)); volume->ImportedRecords++;
        }
    }
    return STATUS_SUCCESS;
}
static NTSTATUS RawCommit(VOLUME *volume, PIRP original, ULONG slot, const UCHAR digest[32], ULONGLONG block) {
    ULONGLONG offset, page; SHDD_STORE_RECORD record; NTSTATUS status;
    if (slot >= volume->SlotCount) return STATUS_INVALID_PARAMETER;
    offset = SHDD_STORE_SECTOR + (ULONGLONG)slot * sizeof(record); page = offset & ~(ULONGLONG)(SHDD_STORE_SECTOR - 1);
    status = RawIo(volume, original, IRP_MJ_READ, volume->Metadata, SHDD_STORE_SECTOR, page);
    if (!NT_SUCCESS(status)) return status;
    RtlZeroMemory(&record, sizeof(record)); record.Block = block;
    RtlCopyMemory(record.Digest, digest, sizeof(record.Digest)); record.Sequence = 1;
    record.Flags = SHDD_STORE_RECORD_VALID; record.RecordHash = ShddStoreHash(&record, FIELD_OFFSET(SHDD_STORE_RECORD, RecordHash));
    RtlCopyMemory(volume->Metadata + (ULONG)(offset - page), &record, sizeof(record));
    return RawIo(volume, original, IRP_MJ_WRITE, volume->Metadata, SHDD_STORE_SECTOR, page);
}
static NTSTATUS RawClear(VOLUME *volume) {
    LONG paused = InterlockedExchange(&volume->Paused, 1); PIRP irp; NTSTATUS status;
    ShddInvalidate(&volume->Epoch);
    while ((irp = IoCsqRemoveNextIrp(&volume->Csq, NULL)) != NULL) ReleaseRead(volume, irp, TRUE, STATUS_SUCCESS, 0);
    ExWaitForRundownProtectionRelease(&volume->Session); volume->SessionLive = FALSE;
    status = RawZeroIndex(volume);
    RtlZeroMemory(volume->Slots, sizeof(SLOT) * volume->SlotCount);
    if (!NT_SUCCESS(status)) ShddDisable(&volume->Epoch);
    ExReInitializeRundownProtection(&volume->Session); volume->SessionLive = TRUE;
    InterlockedExchange(&volume->Paused, paused); return status;
}
static VOID RawClose(VOLUME *volume) {
    if (volume->StoreLocked) {
        /* Best effort only: metadata/data integrity is rechecked after reopen. */
        RawFsControl(volume, FSCTL_UNLOCK_VOLUME); volume->StoreLocked = FALSE;
    }
    if (volume->RawDevice) { ObDereferenceObject(volume->RawDevice); volume->RawDevice = NULL; }
    if (volume->BackingFile) { ObDereferenceObject(volume->BackingFile); volume->BackingFile = NULL; }
    if (volume->Backing) { ZwClose(volume->Backing); volume->Backing = NULL; }
    if (volume->MetadataAllocation) { ExFreePoolWithTag(volume->MetadataAllocation, TAG); volume->MetadataAllocation = NULL; volume->Metadata = NULL; }
    RtlZeroMemory(&volume->StoreHeader, sizeof(volume->StoreHeader));
}
