/* The single reader owns this reusable SHA-256 object. Provider code and hash
 * workspace are nonpaged; provider selection/allocation occurs before attach. */
static NTSTATUS Digest(VOLUME *volume, const UCHAR *bytes, ULONG length, UCHAR result[32]) {
    NTSTATUS status;
    if(!volume->DigestHash)return STATUS_DEVICE_NOT_READY;
    status=BCryptHashData(volume->DigestHash,(PUCHAR)bytes,length,0);
    if(NT_SUCCESS(status))status=BCryptFinishHash(volume->DigestHash,result,32,0);
    return status;
}
static VOID DigestClose(VOLUME *volume) {
    if(volume->DigestHash){BCryptDestroyHash(volume->DigestHash);volume->DigestHash=NULL;}
    if(volume->DigestObject){ExFreePoolWithTag(volume->DigestObject,TAG);volume->DigestObject=NULL;}
    if(volume->DigestAlgorithm){BCryptCloseAlgorithmProvider(volume->DigestAlgorithm,0);volume->DigestAlgorithm=NULL;}
}
static NTSTATUS DigestOpen(VOLUME *volume) {
    ULONG length=0,returned=0;NTSTATUS status;UCHAR actual[32];
    static const UCHAR abc[3]={'a','b','c'};
    static const UCHAR known[32]={0xba,0x78,0x16,0xbf,0x8f,0x01,0xcf,0xea,0x41,0x41,0x40,0xde,0x5d,0xae,0x22,0x23,0xb0,0x03,0x61,0xa3,0x96,0x17,0x7a,0x9c,0xb4,0x10,0xff,0x61,0xf2,0x00,0x15,0xad};
    UCHAR empty[32];
    status=BCryptOpenAlgorithmProvider(&volume->DigestAlgorithm,BCRYPT_SHA256_ALGORITHM,MS_PRIMITIVE_PROVIDER,BCRYPT_PROV_DISPATCH);
    if(!NT_SUCCESS(status))return status;
    status=BCryptGetProperty(volume->DigestAlgorithm,BCRYPT_OBJECT_LENGTH,(PUCHAR)&length,sizeof(length),&returned,0);
    if(!NT_SUCCESS(status))return status;
    if(returned!=sizeof(length)||!length||length>65536)return STATUS_NOT_SUPPORTED;
    volume->DigestObject=ExAllocatePool2(POOL_FLAG_NON_PAGED,length,TAG);
    if(!volume->DigestObject)return STATUS_INSUFFICIENT_RESOURCES;
    status=BCryptCreateHash(volume->DigestAlgorithm,&volume->DigestHash,volume->DigestObject,length,NULL,0,BCRYPT_HASH_REUSABLE_FLAG);
    if(!NT_SUCCESS(status))return status;
    status=Digest(volume,abc,sizeof(abc),actual);
    if(!NT_SUCCESS(status))return status;
    if(!ShddDigestEqual(actual,known))return STATUS_DATA_ERROR;
    /* Check reusable-object reset against the separately tested reference. */
    ShddDigest(abc,0,empty);status=Digest(volume,abc,0,actual);
    return !NT_SUCCESS(status)?status:(ShddDigestEqual(actual,empty)?STATUS_SUCCESS:STATUS_DATA_ERROR);
}
