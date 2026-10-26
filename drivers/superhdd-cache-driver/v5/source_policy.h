#pragma once
/* Pure policy shared with the native C tests. Bus constants follow STORAGE_BUS_TYPE. */
#define SHDD_DATA_ACK 0x44415441u
#define SHDD_VM_ACK 0x564d5445u
typedef struct _SHDD_SOURCE_FACTS {
    unsigned int Bus, Removable, SerialPresent, SeekKnown, IncursSeek;
    unsigned int SystemRole, SpecialUsage, SingleExtent, ExpectedDisk, ActualDisk;
    unsigned long long Offset, Length, ExpectedOffset, ExpectedLength;
} SHDD_SOURCE_FACTS;
static int ShddDirectSourceAllowed(const SHDD_SOURCE_FACTS *f, unsigned int ack, int vmBuild) {
    int vm = vmBuild && ack == SHDD_VM_ACK && (f->Bus == 1u || f->Bus == 10u);
    if (!f->SingleExtent || f->Removable || f->SystemRole || f->SpecialUsage
        || f->ActualDisk != f->ExpectedDisk || !f->Offset || f->Length < 65536u
        || f->Offset != f->ExpectedOffset || f->Length != f->ExpectedLength
        || f->Length > 0x7fffffffffffffffULL || f->Offset > 0x7fffffffffffffffULL-f->Length) return 0;
    if (vm) return 1;
    return !vmBuild && ack == SHDD_DATA_ACK && (f->Bus == 3u || f->Bus == 11u)
        && f->SerialPresent && f->SeekKnown && f->IncursSeek == 1u;
}
static int ShddDirectParentAllowed(const SHDD_SOURCE_FACTS *f, unsigned int sourceDisk,
    unsigned int ack, int vmBuild) {
    int vm = vmBuild && ack == SHDD_VM_ACK && (f->Bus == 1u || f->Bus == 10u);
    if (!f->SingleExtent || f->Removable || f->SystemRole || f->SpecialUsage
        || f->ActualDisk != f->ExpectedDisk || f->ActualDisk == sourceDisk || !f->Offset || !f->Length
        || f->Length > 0x7fffffffffffffffULL || f->Offset > 0x7fffffffffffffffULL-f->Length) return 0;
    return vm || (!vmBuild && ack == SHDD_DATA_ACK && (f->Bus == 3u || f->Bus == 11u || f->Bus == 17u)
        && f->SerialPresent && f->SeekKnown && f->IncursSeek == 0u);
}
