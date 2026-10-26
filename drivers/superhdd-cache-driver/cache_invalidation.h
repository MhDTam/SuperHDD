#pragma once
/* Shared bounded DSM parser and dirty-range history. All callers serialize
 * history access. Unknown/malformed/whole-device operations invalidate globally. */
#define SHDD_DIRTY_RANGE_LIMIT 32u
#define SHDD_DSM_RANGE_LIMIT 16u
typedef struct _SHDD_DIRTY_RANGE { ULONGLONG First, Last; LONG64 Stamp; } SHDD_DIRTY_RANGE;
static __forceinline BOOLEAN ShddDsmReadOnly(const VOID *buffer, ULONG bytes) {
    DEVICE_MANAGE_DATA_SET_ATTRIBUTES header;
    if (!buffer || bytes < sizeof(header)) return FALSE;
    RtlCopyMemory(&header, buffer, sizeof(header));
    return header.Size == sizeof(header) && IsDsmActionNonDestructive(header.Action);
}
static __forceinline BOOLEAN ShddTrimRanges(const VOID *buffer, ULONG bytes, ULONGLONG sourceBytes,
        SHDD_DIRTY_RANGE ranges[SHDD_DSM_RANGE_LIMIT], ULONG *count) {
    DEVICE_MANAGE_DATA_SET_ATTRIBUTES header; ULONG i, number;
    *count=0;
    if (!buffer || bytes < sizeof(header)) return FALSE;
    RtlCopyMemory(&header,buffer,sizeof(header));
    if (header.Size!=sizeof(header) || header.Action!=DeviceDsmAction_Trim
        || (header.Flags & ~(0x80000000u|0x40000000u)) || header.ParameterBlockOffset || header.ParameterBlockLength
        || header.DataSetRangesOffset<sizeof(header) || header.DataSetRangesOffset%8
        || header.DataSetRangesOffset>bytes || !header.DataSetRangesLength
        || header.DataSetRangesLength>bytes-header.DataSetRangesOffset
        || header.DataSetRangesLength%sizeof(DEVICE_DATA_SET_RANGE)) return FALSE;
    number=header.DataSetRangesLength/sizeof(DEVICE_DATA_SET_RANGE);
    if(number>SHDD_DSM_RANGE_LIMIT)return FALSE;
    for(i=0;i<number;i++){
        DEVICE_DATA_SET_RANGE input;
        RtlCopyMemory(&input,(const UCHAR*)buffer+header.DataSetRangesOffset+i*sizeof(input),sizeof(input));
        if(input.StartingOffset<0 || !input.LengthInBytes || (ULONGLONG)input.StartingOffset>=sourceBytes
            || input.LengthInBytes>sourceBytes-(ULONGLONG)input.StartingOffset)return FALSE;
        ranges[i].First=(ULONGLONG)input.StartingOffset/SHDD_BLOCK_BYTES;
        ranges[i].Last=((ULONGLONG)input.StartingOffset+input.LengthInBytes-1)/SHDD_BLOCK_BYTES;
        ranges[i].Stamp=0;
    }
    *count=number;return TRUE;
}
static __forceinline BOOLEAN ShddRememberRange(SHDD_DIRTY_RANGE history[SHDD_DIRTY_RANGE_LIMIT],ULONG *count,
        const SHDD_DIRTY_RANGE *range,LONG64 stamp){
    ULONG i;
    for(i=0;i<*count;i++){
        if(range->First<=history[i].Last+1 && history[i].First<=range->Last+1){
            if(range->First<history[i].First)history[i].First=range->First;
            if(range->Last>history[i].Last)history[i].Last=range->Last;
            if(stamp>history[i].Stamp)history[i].Stamp=stamp;
            return TRUE;
        }
    }
    if(*count==SHDD_DIRTY_RANGE_LIMIT){*count=0;return FALSE;}
    history[*count]=*range;history[*count].Stamp=stamp;(*count)++;return TRUE;
}
static __forceinline BOOLEAN ShddRangesTrust(const SHDD_DIRTY_RANGE *history,ULONG count,
        ULONGLONG block,ULONGLONG published){
    ULONG i;
    for(i=0;i<count;i++)if(block>=history[i].First && block<=history[i].Last
        && published<(ULONGLONG)history[i].Stamp)return FALSE;
    return TRUE;
}
