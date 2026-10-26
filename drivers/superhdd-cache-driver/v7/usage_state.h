#pragma once
/* Pure transition policy, also compiled by the host regression tests.
 * Serialize notifications in the driver and commit/undo on lower completion. */
#define SHDD_USAGE_PAGING 1u
#define SHDD_USAGE_HIBERNATION 2u
#define SHDD_USAGE_DUMP 3u
typedef struct _SHDD_USAGE_COUNTS {
    unsigned int Paging, Hibernation, Dump;
} SHDD_USAGE_COUNTS;
static int ShddUsageNext(const SHDD_USAGE_COUNTS *before, unsigned int type,
        int inPath, SHDD_USAGE_COUNTS *after) {
    unsigned int *count;
    *after=*before;
    switch(type) {
        case SHDD_USAGE_PAGING: count=&after->Paging;break;
        case SHDD_USAGE_HIBERNATION: count=&after->Hibernation;break;
        case SHDD_USAGE_DUMP: count=&after->Dump;break;
        default:return 0;
    }
    if(inPath) { if(*count>=0x7fffffffu)return 0;(*count)++; }
    else { if(!*count)return 0;(*count)--; }
    return 1;
}
static int ShddUsagePresent(const SHDD_USAGE_COUNTS *counts) {
    return counts->Paging || counts->Hibernation || counts->Dump;
}
static int ShddUsageBlocksCache(const SHDD_USAGE_COUNTS *counts) {
    /* Paging IRPs always bypass this first coexistence implementation. */
    return counts->Hibernation || counts->Dump;
}
