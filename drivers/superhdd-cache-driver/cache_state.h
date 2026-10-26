#pragma once
/* Shared by the WDM driver and its Windows user-mode C logic tests.
 * Interlocked operations provide ordering; these tests do not emulate kernel I/O.
 */
#define SHDD_SECTOR_ALIGNMENT 4096u
#define SHDD_MAX_READ_BYTES (1024u * 1024u)
#define SHDD_MAX_PENDING_READS 16
#define SHDD_MAX_CAPACITY_BYTES ((25000000000ULL / SHDD_BLOCK_BYTES) * SHDD_BLOCK_BYTES)
static __forceinline BOOLEAN ShddCapacityValid(ULONGLONG capacity) {
    return capacity >= SHDD_BLOCK_BYTES && capacity <= SHDD_MAX_CAPACITY_BYTES
        && capacity % SHDD_BLOCK_BYTES == 0;
}
static __forceinline ULONG ShddSliceBytes(ULONGLONG position, ULONG remaining) {
    ULONG available = SHDD_BLOCK_BYTES - (ULONG)(position % SHDD_BLOCK_BYTES);
    return remaining < available ? remaining : available;
}

typedef struct _SHDD_EPOCH {
    volatile LONG64 Generation;
    volatile LONG ActiveMutations;
    volatile LONG Disabled;
} SHDD_EPOCH;

static __forceinline LONG ShddAtomicRead(volatile LONG *value) {
    return InterlockedCompareExchange(value, 0, 0);
}
static __forceinline LONG64 ShddGeneration(SHDD_EPOCH *state) {
    return InterlockedCompareExchange64(&state->Generation, 0, 0);
}
/* A mutation changes the read epoch globally, while only its affected slots
 * lose trust. An older racing fill cannot revive a slot invalidated later. */
static __forceinline VOID ShddAdvanceFloor(volatile LONG64 *floor, LONG64 value) {
    LONG64 previous = InterlockedCompareExchange64(floor, 0, 0);
    while (previous < value) {
        LONG64 observed = InterlockedCompareExchange64(floor, value, previous);
        if (observed == previous) return;
        previous = observed;
    }
}
static __forceinline BOOLEAN ShddTrustValid(ULONGLONG published, LONG64 slotFloor, LONG64 globalFloor) {
    return published > 0 && published <= 0x7fffffffffffffffULL
        && published >= (ULONGLONG)slotFloor && published >= (ULONGLONG)globalFloor;
}
static __forceinline VOID ShddInvalidate(SHDD_EPOCH *state) {
    /* Zero is reserved for empty slots. Never reuse old epochs after overflow. */
    LONG64 previous = ShddGeneration(state);
    for (;;) {
        LONG64 observed;
        if (previous <= 0 || previous == 0x7fffffffffffffffLL) {
            InterlockedExchange(&state->Disabled, 1);
            return;
        }
        observed = InterlockedCompareExchange64(&state->Generation, previous + 1, previous);
        if (observed == previous) return;
        previous = observed;
    }
}
static __forceinline VOID ShddMutationBegin(SHDD_EPOCH *state) {
    InterlockedIncrement(&state->ActiveMutations);
    ShddInvalidate(state);
}
static __forceinline VOID ShddMutationEnd(SHDD_EPOCH *state) {
    /* Invalidate again on completion, even on failed or cancelled mutations. */
    ShddInvalidate(state);
    InterlockedDecrement(&state->ActiveMutations);
}
static __forceinline VOID ShddDisable(SHDD_EPOCH *state) {
    InterlockedExchange(&state->Disabled, 1);
    ShddInvalidate(state);
}
static __forceinline BOOLEAN ShddEpochValid(SHDD_EPOCH *state, LONG64 generation) {
    return generation > 0 && !ShddAtomicRead(&state->Disabled)
        && !ShddAtomicRead(&state->ActiveMutations)
        && generation == ShddGeneration(state)
        && !ShddAtomicRead(&state->ActiveMutations)
        && !ShddAtomicRead(&state->Disabled);
}
static __forceinline LONG64 ShddCaptureEpoch(SHDD_EPOCH *state) {
    LONG64 generation = ShddGeneration(state);
    return ShddEpochValid(state, generation) ? generation : 0;
}
static __forceinline BOOLEAN ShddReserveRead(volatile LONG *pending) {
    LONG value = ShddAtomicRead(pending);
    for (;;) {
        LONG observed;
        if (value >= SHDD_MAX_PENDING_READS) return FALSE;
        observed = InterlockedCompareExchange(pending, value + 1, value);
        if (observed == value) return TRUE;
        value = observed;
    }
}
static __forceinline BOOLEAN ShddEligibleRead(
    LONGLONG offset, ULONG length, ULONGLONG sourceLength, ULONG mdlBytes,
    BOOLEAN paging
) {
    ULONGLONG end;
    if (paging || offset < 0 || sourceLength > 0x7fffffffffffffffULL
        || !length || length > SHDD_MAX_READ_BYTES
        || length > mdlBytes || length % SHDD_SECTOR_ALIGNMENT
        || (ULONGLONG)offset % SHDD_SECTOR_ALIGNMENT
        || (ULONGLONG)offset > sourceLength
        || length > sourceLength - (ULONGLONG)offset) return FALSE;
    end = (ULONGLONG)offset + length;
    /* A partial last source block passes through without speculative over-read. */
    return end <= (sourceLength / SHDD_BLOCK_BYTES) * SHDD_BLOCK_BYTES;
}
/* Paging is eligible only with the separately locked RAW backend. Never permit
 * a paging request to issue NTFS cache-file I/O through the v3 file backend. */
static __forceinline BOOLEAN ShddEligibleStorageRead(
    LONGLONG offset, ULONG length, ULONGLONG sourceLength, ULONG mdlBytes,
    BOOLEAN paging, BOOLEAN lockedRawBackend
) {
    return ShddEligibleRead(offset,length,sourceLength,mdlBytes,paging && !lockedRawBackend);
}
