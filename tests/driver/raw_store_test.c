#define WIN32_LEAN_AND_MEAN
#include <Windows.h>
#include <winioctl.h>
#include <bcrypt.h>
#include <stdio.h>
#include <limits.h>
#include "../../drivers/superhdd-cache-driver/protocol.h"
#define SHDD_RAW_STORE 1
#include "../../drivers/superhdd-cache-driver/pnp_protocol.h"
#include "../../drivers/superhdd-cache-driver/cache_state.h"
#include "../../drivers/superhdd-cache-driver/raw_store.h"
static ULONG checks;
static VOID Check(BOOLEAN value,int line,const char *expression){checks++;if(!value){fprintf(stderr,"FAIL line %d: %s\n",line,expression);ExitProcess(1);}}
#define CHECK(expression) Check((BOOLEAN)(expression),__LINE__,#expression)
static VOID Digests(VOID) {
    UCHAR *data=HeapAlloc(GetProcessHeap(),0,1048576),actual[32],expected[32],chunked[32];
    const ULONG lengths[]={0,1,55,56,63,64,65,127,128,4096,65536,1048576};
    ULONG i,j;BCRYPT_ALG_HANDLE algorithm=NULL;SHDD_SHA256 context;
    CHECK(data!=NULL);for(i=0;i<1048576;i++)data[i]=(UCHAR)(i*73+11);
    CHECK(BCRYPT_SUCCESS(BCryptOpenAlgorithmProvider(&algorithm,BCRYPT_SHA256_ALGORITHM,NULL,0)));
    for(i=0;i<sizeof(lengths)/sizeof(lengths[0]);i++){
        CHECK(BCRYPT_SUCCESS(BCryptHash(algorithm,NULL,0,data,lengths[i],expected,sizeof(expected))));
        ShddDigest(data,lengths[i],actual);CHECK(ShddDigestEqual(actual,expected));
        ShddShaInit(&context);for(j=0;j<lengths[i];){ULONG take=min(37,lengths[i]-j);ShddShaUpdate(&context,data+j,take);j+=take;}
        ShddShaFinish(&context,chunked);CHECK(ShddDigestEqual(actual,chunked));
    }
    ShddDigest("abc",3,actual);
    {const UCHAR nist[32]={0xba,0x78,0x16,0xbf,0x8f,0x01,0xcf,0xea,0x41,0x41,0x40,0xde,0x5d,0xae,0x22,0x23,0xb0,0x03,0x61,0xa3,0x96,0x17,0x7a,0x9c,0xb4,0x10,0xff,0x61,0xf2,0x00,0x15,0xad};CHECK(ShddDigestEqual(actual,nist));}
    BCryptCloseAlgorithmProvider(algorithm,0);HeapFree(GetProcessHeap(),0,data);
}
static VOID Layouts(VOID) {
    SHDD_STORE_HEADER header,corrupt;ULONGLONG budget;ULONG i,bit;
    CHECK(sizeof(header)==128);CHECK(sizeof(SHDD_STORE_RECORD)==64);
    CHECK(FIELD_OFFSET(SHDD_STORE_HEADER,HeaderHash)==72);
    CHECK(!ShddStoreLayout(0,&header));CHECK(!ShddStoreLayout(65536,&header));
    CHECK(!ShddStoreLayout(ULLONG_MAX,&header));CHECK(!ShddStoreLayout(SHDD_MAX_CAPACITY_BYTES+65536,&header));
    for(budget=2*SHDD_BLOCK_BYTES;budget<=SHDD_MAX_CAPACITY_BYTES;budget+=1048576){
        CHECK(ShddStoreLayout(budget,&header));CHECK(header.TotalBytes<=budget);CHECK(header.DataOffset%4096==0);
        CHECK(header.TotalBytes==header.DataOffset+(ULONGLONG)header.SlotCount*SHDD_BLOCK_BYTES);
        CHECK(header.DataOffset>=4096+(ULONGLONG)header.SlotCount*sizeof(SHDD_STORE_RECORD));
    }
    CHECK(ShddStoreLayout(SHDD_MAX_CAPACITY_BYTES,&header));header.Flags=SHDD_STORE_READY;
    header.SourceLength=64ULL*SHDD_BLOCK_BYTES;header.SourceSerial=42;header.InstanceId[0]=1;header.HeaderHash=ShddHeaderHash(&header);
    CHECK(ShddHeaderValid(&header));
    for(i=0;i<sizeof(header);i++)for(bit=0;bit<8;bit++){corrupt=header;((UCHAR*)&corrupt)[i]^=(UCHAR)(1u<<bit);CHECK(!ShddHeaderValid(&corrupt));}
    corrupt=header;corrupt.DataOffset=0;corrupt.HeaderHash=ShddHeaderHash(&corrupt);CHECK(!ShddHeaderValid(&corrupt));
    corrupt=header;corrupt.SlotCount=0xffffffffu;corrupt.HeaderHash=ShddHeaderHash(&corrupt);CHECK(!ShddHeaderValid(&corrupt));
    CHECK(ShddStoreRange(4096,65536,header.TotalBytes));CHECK(!ShddStoreRange(1,4096,header.TotalBytes));
    CHECK(!ShddStoreRange(4096,1,header.TotalBytes));CHECK(!ShddStoreRange(ULLONG_MAX-4095,8192,header.TotalBytes));
    CHECK(!ShddStoreRange(header.TotalBytes,4096,header.TotalBytes));
    RtlZeroMemory(&header,sizeof(header));header.Magic=SHDD_STORE_MAGIC;header.Version=4;header.HeaderBytes=4096;
    header.RecordBytes=64;header.Flags=SHDD_STORE_BOOTSTRAP;header.InstanceId[0]=7;header.HeaderHash=ShddHeaderHash(&header);
    CHECK(ShddHeaderValid(&header));header.TotalBytes=4096;header.HeaderHash=ShddHeaderHash(&header);CHECK(!ShddHeaderValid(&header));
}
static VOID Records(VOID){
    SHDD_STORE_RECORD record={0},changed,torn;UCHAR original[65536],modified[65536],current[32];ULONG i,bit,cut;
    RtlFillMemory(original,sizeof(original),0x38);RtlCopyMemory(modified,original,sizeof(original));modified[4096]=0x99;
    record.Block=19;record.Sequence=1;record.Flags=SHDD_STORE_RECORD_VALID;
    ShddDigest(original,sizeof(original),record.Digest);record.RecordHash=ShddStoreHash(&record,56);
    CHECK(ShddRecordValid(&record,3,16,64ULL*65536));CHECK(!ShddRecordValid(&record,2,16,64ULL*65536));
    CHECK(!ShddRecordValid(&record,3,0,64ULL*65536));CHECK(!ShddRecordValid(&record,3,16,19ULL*65536));
    for(i=0;i<sizeof(record);i++)for(bit=0;bit<8;bit++){changed=record;((UCHAR*)&changed)[i]^=(UCHAR)(1u<<bit);CHECK(!ShddRecordValid(&changed,3,16,64ULL*65536));}
    ShddDigest(modified,sizeof(modified),current);CHECK(!ShddDigestEqual(current,record.Digest));
    changed=record;RtlCopyMemory(changed.Digest,current,32);changed.Sequence=2;changed.RecordHash=ShddStoreHash(&changed,56);
    for(cut=0;cut<=sizeof(record);cut++){
        torn=record;RtlCopyMemory(&torn,&changed,cut);
        if(ShddRecordValid(&torn,3,16,64ULL*65536)){
            /* Even a structurally valid OLD record must fail current-source validation. */
            CHECK(ShddDigestEqual(torn.Digest,record.Digest)||ShddDigestEqual(torn.Digest,current));
            if(ShddDigestEqual(torn.Digest,record.Digest))CHECK(!ShddDigestEqual(torn.Digest,current));
        }
    }
}
static VOID MarkerFile(const char *path){
    FILE *file=NULL;UCHAR bytes[4096];SHDD_STORE_HEADER header;ULONG i;
    CHECK(fopen_s(&file,path,"rb")==0&&file!=NULL);
    CHECK(fread(bytes,1,sizeof(bytes),file)==sizeof(bytes));CHECK(fgetc(file)==EOF);CHECK(fclose(file)==0);
    RtlCopyMemory(&header,bytes,sizeof(header));CHECK(ShddHeaderValid(&header));CHECK(header.Flags==SHDD_STORE_BOOTSTRAP);
    for(i=sizeof(header);i<sizeof(bytes);i++)CHECK(bytes[i]==0);
}
int main(int argc,char **argv){
    Digests();Layouts();Records();
    CHECK(SHDD_PNP_PROTOCOL_VERSION==4);CHECK(sizeof(SHDD_PNP_STATUS)==208);
    CHECK(FIELD_OFFSET(SHDD_PNP_STATUS,RawBackend)==160);CHECK(FIELD_OFFSET(SHDD_PNP_STATUS,AllocatedBytes)==168);
    CHECK(sizeof(SHDD_ATTACH)==1576);
    CHECK(sizeof(SHDD_CONFIG_DIAGNOSTIC)==48);
    CHECK(ShddEligibleStorageRead(0,4096,65536,4096,TRUE,TRUE));
    CHECK(!ShddEligibleStorageRead(0,4096,65536,4096,TRUE,FALSE));
    CHECK(ShddEligibleStorageRead(0,4096,65536,4096,FALSE,FALSE));
    CHECK(!ShddEligibleStorageRead(1,4096,65536,4096,TRUE,TRUE));
    CHECK(!ShddEligibleStorageRead(0,4096,65536,4095,TRUE,TRUE));
    CHECK(!ShddEligibleStorageRead(65536,4096,65536,4096,TRUE,TRUE));
    CHECK(!ShddEligibleStorageRead(-1,4096,65536,4096,TRUE,TRUE));
    if(argc==2)MarkerFile(argv[1]);else if(argc!=1){fprintf(stderr,"Unexpected arguments\n");return 2;}
    printf("{\"checks\":%lu,\"passed\":true,\"rustMarkerDecoded\":%s,\"scope\":\"Actual shared raw-store codec, layouts and SHA-256 cross-checks against Windows BCrypt; host tests, not kernel I/O\"}\n",checks,argc==2?"true":"false");return 0;
}
