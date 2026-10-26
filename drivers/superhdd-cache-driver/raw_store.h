#pragma once
/* Versioned, bounds-checked on-volume format. No device I/O in this header.
 * Persisted payload is UNTRUSTED until compared with the current source after
 * every new attachment. Checksums detect corruption, not malicious forgery.
 */
#define SHDD_STORE_MAGIC 0x3457415244444853ULL
#define SHDD_STORE_VERSION 4u
#define SHDD_STORE_SECTOR 4096u
#define SHDD_STORE_BOOTSTRAP 1u
#define SHDD_STORE_READY 2u
#define SHDD_STORE_RECORD_VALID 0x54494d43u
typedef struct _SHDD_STORE_HEADER {
    ULONGLONG Magic;
    ULONG Version, HeaderBytes;
    ULONGLONG TotalBytes;
    ULONG SlotCount, RecordBytes;
    ULONGLONG DataOffset, SourceLength;
    ULONG SourceSerial, Flags;
    UCHAR InstanceId[16];
    ULONGLONG HeaderHash;
    ULONGLONG Reserved[6];
} SHDD_STORE_HEADER;
typedef struct _SHDD_STORE_RECORD {
    ULONGLONG Block;
    UCHAR Digest[32];
    ULONGLONG Sequence;
    ULONG Flags, Reserved;
    ULONGLONG RecordHash;
} SHDD_STORE_RECORD;
static __forceinline ULONGLONG ShddStoreHash(const VOID *memory, ULONG length) {
    const UCHAR *bytes = memory; ULONGLONG hash = 0xcbf29ce484222325ULL; ULONG i;
    for (i = 0; i < length; i++) { hash ^= bytes[i]; hash *= 0x100000001b3ULL; }
    return hash;
}
static __forceinline ULONGLONG ShddHeaderHash(const SHDD_STORE_HEADER *header) {
    SHDD_STORE_HEADER copy = *header; copy.HeaderHash = 0;
    return ShddStoreHash(&copy, sizeof(copy));
}
static __forceinline BOOLEAN ShddStoreLayout(ULONGLONG budget, SHDD_STORE_HEADER *header) {
    ULONGLONG slots, metadata, total;
    if (budget < 2ULL * SHDD_BLOCK_BYTES || budget > SHDD_MAX_CAPACITY_BYTES || budget % SHDD_STORE_SECTOR) return FALSE;
    slots = (budget - SHDD_STORE_SECTOR) / (SHDD_BLOCK_BYTES + sizeof(SHDD_STORE_RECORD));
    if (!slots || slots > 0xffffffffu) return FALSE;
    for (;;) {
        metadata = (slots * sizeof(SHDD_STORE_RECORD) + SHDD_STORE_SECTOR - 1) & ~(ULONGLONG)(SHDD_STORE_SECTOR - 1);
        total = SHDD_STORE_SECTOR + metadata + slots * SHDD_BLOCK_BYTES;
        if (total <= budget) break;
        if (!--slots) return FALSE;
    }
    RtlZeroMemory(header, sizeof(*header)); header->Magic = SHDD_STORE_MAGIC;
    header->Version = SHDD_STORE_VERSION; header->HeaderBytes = SHDD_STORE_SECTOR;
    header->TotalBytes = total; header->SlotCount = (ULONG)slots; header->RecordBytes = sizeof(SHDD_STORE_RECORD);
    header->DataOffset = SHDD_STORE_SECTOR + metadata; return TRUE;
}
static __forceinline BOOLEAN ShddHeaderValid(const SHDD_STORE_HEADER *header) {
    ULONG i; UCHAR instance = 0;
    if (header->Magic != SHDD_STORE_MAGIC || header->Version != SHDD_STORE_VERSION
        || header->HeaderBytes != SHDD_STORE_SECTOR || header->RecordBytes != sizeof(SHDD_STORE_RECORD)
        || header->HeaderHash != ShddHeaderHash(header)) return FALSE;
    for (i = 0; i < 6; i++) if (header->Reserved[i]) return FALSE;
    for (i = 0; i < sizeof(header->InstanceId); i++) instance |= header->InstanceId[i];
    if (!instance) return FALSE;
    if (header->Flags == SHDD_STORE_BOOTSTRAP)
        return !header->TotalBytes && !header->SlotCount && !header->DataOffset && !header->SourceLength && !header->SourceSerial;
    if (header->Flags != SHDD_STORE_READY || !header->SlotCount || header->SourceLength < SHDD_BLOCK_BYTES
        || header->SourceLength > 0x7fffffffffffffffULL) return FALSE;
    {
        ULONGLONG metadata = ((ULONGLONG)header->SlotCount * sizeof(SHDD_STORE_RECORD) + SHDD_STORE_SECTOR - 1) & ~(ULONGLONG)(SHDD_STORE_SECTOR - 1);
        ULONGLONG total = SHDD_STORE_SECTOR + metadata + (ULONGLONG)header->SlotCount * SHDD_BLOCK_BYTES;
        return header->DataOffset == SHDD_STORE_SECTOR + metadata && header->TotalBytes == total && total <= SHDD_MAX_CAPACITY_BYTES;
    }
}
static __forceinline BOOLEAN ShddRecordValid(const SHDD_STORE_RECORD *record, ULONG slot, ULONG slots, ULONGLONG sourceLength) {
    return slots && record->Flags == SHDD_STORE_RECORD_VALID && !record->Reserved && record->Sequence
        && record->Block < sourceLength / SHDD_BLOCK_BYTES && record->Block % slots == slot
        && record->RecordHash == ShddStoreHash(record, FIELD_OFFSET(SHDD_STORE_RECORD, RecordHash));
}
static __forceinline BOOLEAN ShddStoreRange(ULONGLONG offset, ULONG length, ULONGLONG total) {
    return length && !(offset % SHDD_STORE_SECTOR) && !(length % SHDD_STORE_SECTOR)
        && offset <= total && length <= total - offset;
}
/* Small SHA-256 implementation shared with host known-vector/fault tests.
 * Input blocks are transient; the cache never keeps a RAM payload tier. */
typedef struct _SHDD_SHA256 { ULONG State[8]; ULONGLONG Bits; UCHAR Buffer[64]; ULONG Used; } SHDD_SHA256;
static __forceinline ULONG ShddRotate(ULONG n, ULONG count) { return (n >> count) | (n << (32 - count)); }
static VOID ShddShaBlock(SHDD_SHA256 *context, const UCHAR *input) {
    static const ULONG constants[64] = {
        0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
        0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
        0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
        0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
        0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
        0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
        0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
        0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2};
    ULONG words[64], a,b,c,d,e,f,g,h,i,t1,t2;
    for(i=0;i<16;i++) words[i]=((ULONG)input[i*4]<<24)|((ULONG)input[i*4+1]<<16)|((ULONG)input[i*4+2]<<8)|input[i*4+3];
    for(i=16;i<64;i++) words[i]=words[i-16]+(ShddRotate(words[i-15],7)^ShddRotate(words[i-15],18)^(words[i-15]>>3))
        +words[i-7]+(ShddRotate(words[i-2],17)^ShddRotate(words[i-2],19)^(words[i-2]>>10));
    a=context->State[0];b=context->State[1];c=context->State[2];d=context->State[3];
    e=context->State[4];f=context->State[5];g=context->State[6];h=context->State[7];
    for(i=0;i<64;i++){
        t1=h+(ShddRotate(e,6)^ShddRotate(e,11)^ShddRotate(e,25))+((e&f)^((~e)&g))+constants[i]+words[i];
        t2=(ShddRotate(a,2)^ShddRotate(a,13)^ShddRotate(a,22))+((a&b)^(a&c)^(b&c));
        h=g;g=f;f=e;e=d+t1;d=c;c=b;b=a;a=t1+t2;
    }
    context->State[0]+=a;context->State[1]+=b;context->State[2]+=c;context->State[3]+=d;
    context->State[4]+=e;context->State[5]+=f;context->State[6]+=g;context->State[7]+=h;
}
static VOID ShddShaInit(SHDD_SHA256 *context) {
    static const ULONG initial[8]={0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19};
    RtlZeroMemory(context,sizeof(*context));RtlCopyMemory(context->State,initial,sizeof(initial));
}
static VOID ShddShaUpdate(SHDD_SHA256 *context,const UCHAR *bytes,ULONG length) {
    ULONG i;context->Bits+=(ULONGLONG)length*8;
    for(i=0;i<length;i++){context->Buffer[context->Used++]=bytes[i];if(context->Used==64){ShddShaBlock(context,context->Buffer);context->Used=0;}}
}
static VOID ShddShaFinish(SHDD_SHA256 *context,UCHAR digest[32]) {
    ULONG i;ULONGLONG bits=context->Bits;context->Buffer[context->Used++]=0x80;
    if(context->Used>56){while(context->Used<64)context->Buffer[context->Used++]=0;ShddShaBlock(context,context->Buffer);context->Used=0;}
    while(context->Used<56)context->Buffer[context->Used++]=0;
    for(i=0;i<8;i++)context->Buffer[63-i]=(UCHAR)(bits>>(i*8));ShddShaBlock(context,context->Buffer);
    for(i=0;i<32;i++)digest[i]=(UCHAR)(context->State[i/4]>>(24-(i%4)*8));
}
static VOID ShddDigest(const VOID *bytes,ULONG length,UCHAR digest[32]) {
    SHDD_SHA256 context;ShddShaInit(&context);ShddShaUpdate(&context,bytes,length);ShddShaFinish(&context,digest);
}
static __forceinline BOOLEAN ShddDigestEqual(const UCHAR left[32],const UCHAR right[32]) {
    UCHAR difference=0;ULONG i;for(i=0;i<32;i++)difference|=left[i]^right[i];return !difference;
}
