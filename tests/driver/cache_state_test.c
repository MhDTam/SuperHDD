/* Executes the actual shared epoch/queue/range helpers, without loading a driver. */
#define WIN32_LEAN_AND_MEAN
#include <Windows.h>
#include <winioctl.h>
#include <stdio.h>
#include <limits.h>
#include "../../drivers/superhdd-cache-driver/protocol.h"
#include "../../drivers/superhdd-cache-driver/cache_state.h"
#include "../../drivers/superhdd-cache-driver/cache_invalidation.h"
#include "../../drivers/superhdd-cache-driver/pnp_state.h"
#include "../../drivers/superhdd-cache-driver/pnp_protocol.h"
static int checks=0;
static VOID Check(BOOLEAN condition,int line,const char *expression){checks++;if(!condition){fprintf(stderr,"FAIL line %d: %s\n",line,expression);ExitProcess(1);}}
#define CHECK(condition) Check((BOOLEAN)(condition),__LINE__,#condition)
static SHDD_EPOCH Fresh(VOID){SHDD_EPOCH state={0};state.Generation=1;return state;}
static VOID EpochTests(VOID){
    SHDD_EPOCH state=Fresh();LONG64 before=ShddCaptureEpoch(&state),during;
    CHECK(before==1);ShddMutationBegin(&state);during=ShddGeneration(&state);
    CHECK(state.ActiveMutations==1);CHECK(!ShddCaptureEpoch(&state));
    CHECK(!ShddEpochValid(&state,before));CHECK(!ShddEpochValid(&state,during));
    ShddMutationEnd(&state);
    CHECK(!state.ActiveMutations);CHECK(ShddCaptureEpoch(&state)==3);
    CHECK(!ShddEpochValid(&state,during));CHECK(!ShddEpochValid(&state,before));
    ShddMutationBegin(&state);ShddMutationBegin(&state);ShddMutationEnd(&state);
    CHECK(state.ActiveMutations==1);CHECK(!ShddCaptureEpoch(&state));
    ShddMutationEnd(&state);CHECK(state.ActiveMutations==0);CHECK(ShddCaptureEpoch(&state)>0);
    before=ShddCaptureEpoch(&state);ShddInvalidate(&state);CHECK(!ShddEpochValid(&state,before));
    before=ShddCaptureEpoch(&state);ShddDisable(&state);CHECK(!ShddEpochValid(&state,before));CHECK(!ShddCaptureEpoch(&state));
    state=Fresh();state.Generation=LLONG_MAX;ShddInvalidate(&state);
    CHECK(state.Disabled);CHECK(!ShddCaptureEpoch(&state));
    state.Generation=1;CHECK(!ShddCaptureEpoch(&state));
}
static VOID RangeTests(VOID){
    ULONGLONG source=8ULL*SHDD_BLOCK_BYTES;
    CHECK(ShddEligibleRead(0,4096,source,4096,FALSE));
    CHECK(ShddEligibleRead(61440,12288,source,12288,FALSE));
    CHECK(ShddEligibleRead(0,SHDD_MAX_READ_BYTES,16ULL*SHDD_BLOCK_BYTES,SHDD_MAX_READ_BYTES,FALSE));
    CHECK(!ShddEligibleRead(-4096,4096,source,4096,FALSE));
    CHECK(!ShddEligibleRead(0,0,source,4096,FALSE));
    CHECK(!ShddEligibleRead(1,4096,source,4096,FALSE));
    CHECK(!ShddEligibleRead(0,512,source,4096,FALSE));
    CHECK(!ShddEligibleRead(0,4096,source,4095,FALSE));
    CHECK(!ShddEligibleRead(0,4096,source,4096,TRUE));
    CHECK(!ShddEligibleRead((LONGLONG)source,4096,source,4096,FALSE));
    CHECK(!ShddEligibleRead(0,SHDD_MAX_READ_BYTES+4096,source,SHDD_MAX_READ_BYTES+4096,FALSE));
    CHECK(!ShddEligibleRead(0,4096,4096,4096,FALSE));
    CHECK(!ShddEligibleRead((LONGLONG)source,4096,source+4096,4096,FALSE));
    CHECK(!ShddEligibleRead(LLONG_MAX-4095,8192,ULLONG_MAX,8192,FALSE));
    CHECK(ShddCapacityValid(SHDD_BLOCK_BYTES));CHECK(ShddCapacityValid(SHDD_MAX_CAPACITY_BYTES));
    CHECK(!ShddCapacityValid(0));CHECK(!ShddCapacityValid(SHDD_BLOCK_BYTES+1));
    CHECK(!ShddCapacityValid(SHDD_MAX_CAPACITY_BYTES+SHDD_BLOCK_BYTES));
    CHECK(SHDD_MAX_CAPACITY_BYTES<=25000000000ULL);
    CHECK(ShddSliceBytes(61440,12288)==4096);CHECK(ShddSliceBytes(65536,8192)==8192);
    CHECK(ShddSliceBytes(0,131072)==65536);CHECK(ShddSliceBytes(1,65536)==65535);
    CHECK(sizeof(SHDD_STATUS)==128);CHECK(FIELD_OFFSET(SHDD_STATUS,CapacityBytes)==80);
    CHECK(FIELD_OFFSET(SHDD_STATUS,ReadBypasses)==112);CHECK(SHDD_PROTOCOL_VERSION==2);
}
typedef struct _QUEUE_TEST {volatile LONG Pending,Attempts,Accepted;HANDLE Release;} QUEUE_TEST;
static DWORD WINAPI QueueThread(PVOID context){
    QUEUE_TEST *test=context;BOOLEAN reserved=ShddReserveRead(&test->Pending);
    if(reserved)InterlockedIncrement(&test->Accepted);
    InterlockedIncrement(&test->Attempts);
    if(reserved){WaitForSingleObject(test->Release,5000);InterlockedDecrement(&test->Pending);}
    return 0;
}
static VOID QueueTests(VOID){
    QUEUE_TEST state={0};HANDLE threads[32];ULONG index;ULONGLONG deadline;
    state.Release=CreateEventW(NULL,TRUE,FALSE,NULL);CHECK(state.Release!=NULL);
    for(index=0;index<32;index++){threads[index]=CreateThread(NULL,0,QueueThread,&state,0,NULL);CHECK(threads[index]!=NULL);}
    deadline=GetTickCount64()+4000;
    while(ShddAtomicRead(&state.Attempts)!=32&&GetTickCount64()<deadline)Sleep(1);
    CHECK(state.Attempts==32);CHECK(state.Accepted==SHDD_MAX_PENDING_READS);CHECK(state.Pending==SHDD_MAX_PENDING_READS);
    CHECK(!ShddReserveRead(&state.Pending));SetEvent(state.Release);
    CHECK(WaitForMultipleObjects(32,threads,TRUE,5000)==WAIT_OBJECT_0);
    CHECK(state.Pending==0);CHECK(ShddReserveRead(&state.Pending));InterlockedDecrement(&state.Pending);
    for(index=0;index<32;index++)CloseHandle(threads[index]);CloseHandle(state.Release);
}
static VOID PnpTests(VOID){
    SHDD_EPOCH epoch=Fresh();ULONG bit,index;WCHAR path[520]={0};
    CHECK(!ShddPnpConfigurable(0));CHECK(ShddPnpConfigurable(SHDD_PNP_STARTED));
    for(bit=SHDD_PNP_STOPPED;bit<=SHDD_PNP_POWER_CHANGED;bit<<=1){
        CHECK(!ShddPnpConfigurable(SHDD_PNP_STARTED|bit));
        CHECK(!ShddPnpReadable(SHDD_PNP_STARTED|bit,1,0,&epoch));
    }
    CHECK(ShddPnpReadable(SHDD_PNP_STARTED,1,0,&epoch));
    CHECK(!ShddPnpReadable(SHDD_PNP_STARTED,0,0,&epoch));
    CHECK(!ShddPnpReadable(SHDD_PNP_STARTED,1,1,&epoch));
    ShddDisable(&epoch);CHECK(!ShddPnpReadable(SHDD_PNP_STARTED,1,0,&epoch));
    wcscpy_s(path,520,L"\\Device\\HarddiskVolume42");CHECK(ShddWirePathValid(path,260,FALSE));
    CHECK(!ShddWirePathValid(path,520,TRUE));
    wcscpy_s(path,520,L"\\??\\V:\\cache\\payload.bin");CHECK(ShddWirePathValid(path,520,TRUE));
    wcscpy_s(path,520,L"\\??\\Volume{test}\\cache\\payload.bin");CHECK(ShddWirePathValid(path,520,TRUE));
    wcscpy_s(path,520,L"\\??\\V:\\cache\\payload.bin:stream");CHECK(!ShddWirePathValid(path,520,TRUE));
    wcscpy_s(path,520,L"\\??\\V:\\cache\\..\\payload.bin");CHECK(!ShddWirePathValid(path,520,TRUE));
    wcscpy_s(path,520,L"\\Device\\HarddiskVolume42\\");CHECK(!ShddWirePathValid(path,260,FALSE));
    wcscpy_s(path,520,L"\\Device\\HarddiskVolume42/evil");CHECK(!ShddWirePathValid(path,260,FALSE));
    for(index=0;index<520;index++)path[index]=L'A';CHECK(!ShddWirePathValid(path,520,TRUE));
    CHECK(sizeof(SHDD_PNP_STATUS)==160);CHECK(FIELD_OFFSET(SHDD_PNP_STATUS,VolumeCount)==128);
    CHECK(FIELD_OFFSET(SHDD_PNP_STATUS,CancelledReads)==144);CHECK(SHDD_PNP_PROTOCOL_VERSION==3);
    CHECK(!(SHDD_BACKING_ACCESS&FILE_APPEND_DATA));CHECK(SHDD_BACKING_ACCESS&FILE_WRITE_DATA);
}
static VOID TrustTests(VOID){
    volatile LONG64 slotFloor=0,globalFloor=0;
    CHECK(!ShddTrustValid(0,0,0));CHECK(ShddTrustValid(1,0,0));
    ShddAdvanceFloor(&slotFloor,12);CHECK(slotFloor==12);
    CHECK(!ShddTrustValid(11,slotFloor,globalFloor));CHECK(ShddTrustValid(12,slotFloor,globalFloor));
    ShddAdvanceFloor(&slotFloor,4);CHECK(slotFloor==12);
    CHECK(!ShddTrustValid(3,slotFloor,globalFloor));
    CHECK(ShddTrustValid(3,0,globalFloor));
    ShddAdvanceFloor(&globalFloor,20);CHECK(!ShddTrustValid(12,slotFloor,globalFloor));
    CHECK(ShddTrustValid(20,slotFloor,globalFloor));
    CHECK(!ShddTrustValid(0xffffffffffffffffULL,0,0));
    ShddAdvanceFloor(&globalFloor,19);CHECK(globalFloor==20);
}
static VOID DsmTests(VOID){
    UCHAR bytes[64]={0};DEVICE_MANAGE_DATA_SET_ATTRIBUTES h={0};DEVICE_DATA_SET_RANGE r={0};
    SHDD_DIRTY_RANGE parsed[SHDD_DSM_RANGE_LIMIT],history[SHDD_DIRTY_RANGE_LIMIT]={0},extra={0};ULONG count=0,n=0,i;
    h.Size=sizeof(h);h.Action=DeviceDsmAction_Trim;h.DataSetRangesOffset=32;h.DataSetRangesLength=sizeof(r);
    r.StartingOffset=4096;r.LengthInBytes=65536;RtlCopyMemory(bytes,&h,sizeof(h));RtlCopyMemory(bytes+32,&r,sizeof(r));
    CHECK(ShddTrimRanges(bytes,sizeof(bytes),1048576,parsed,&count));CHECK(count==1);
    CHECK(parsed[0].First==0&&parsed[0].Last==1);CHECK(!ShddDsmReadOnly(bytes,sizeof(bytes)));
    CHECK(ShddRememberRange(history,&n,&parsed[0],10));CHECK(n==1);
    CHECK(!ShddRangesTrust(history,n,0,9));CHECK(!ShddRangesTrust(history,n,1,9));
    CHECK(ShddRangesTrust(history,n,2,1));CHECK(ShddRangesTrust(history,n,0,10));
    extra.First=2;extra.Last=3;CHECK(ShddRememberRange(history,&n,&extra,12));CHECK(n==1);
    CHECK(!ShddRangesTrust(history,n,3,11));CHECK(ShddRangesTrust(history,n,4,1));
    h.Flags=DEVICE_DSM_FLAG_ENTIRE_DATA_SET_RANGE;RtlCopyMemory(bytes,&h,sizeof(h));CHECK(!ShddTrimRanges(bytes,sizeof(bytes),1048576,parsed,&count));
    h.Flags=0;h.DataSetRangesOffset=31;RtlCopyMemory(bytes,&h,sizeof(h));CHECK(!ShddTrimRanges(bytes,sizeof(bytes),1048576,parsed,&count));
    h.DataSetRangesOffset=32;h.DataSetRangesLength=0xffffffffu;RtlCopyMemory(bytes,&h,sizeof(h));CHECK(!ShddTrimRanges(bytes,sizeof(bytes),1048576,parsed,&count));
    h.DataSetRangesLength=sizeof(r);h.ParameterBlockLength=4;RtlCopyMemory(bytes,&h,sizeof(h));CHECK(!ShddTrimRanges(bytes,sizeof(bytes),1048576,parsed,&count));
    h.ParameterBlockLength=0;h.Action=DeviceDsmAction_Allocation;RtlCopyMemory(bytes,&h,sizeof(h));CHECK(ShddDsmReadOnly(bytes,sizeof(bytes)));
    CHECK(!ShddTrimRanges(bytes,sizeof(bytes),1048576,parsed,&count));CHECK(!ShddDsmReadOnly(bytes,8));
    h.Action=DeviceDsmAction_Trim;RtlCopyMemory(bytes,&h,sizeof(h));r.StartingOffset=-1;RtlCopyMemory(bytes+32,&r,sizeof(r));CHECK(!ShddTrimRanges(bytes,sizeof(bytes),1048576,parsed,&count));
    r.StartingOffset=1048576;r.LengthInBytes=1;RtlCopyMemory(bytes+32,&r,sizeof(r));CHECK(!ShddTrimRanges(bytes,sizeof(bytes),1048576,parsed,&count));
    r.StartingOffset=0;r.LengthInBytes=0xffffffffffffffffULL;RtlCopyMemory(bytes+32,&r,sizeof(r));CHECK(!ShddTrimRanges(bytes,sizeof(bytes),1048576,parsed,&count));
    n=0;for(i=0;i<SHDD_DIRTY_RANGE_LIMIT;i++){extra.First=extra.Last=(ULONGLONG)i*4;CHECK(ShddRememberRange(history,&n,&extra,20));}
    extra.First=extra.Last=9999;CHECK(!ShddRememberRange(history,&n,&extra,21));CHECK(n==0);
}
int main(VOID){
    EpochTests();RangeTests();QueueTests();PnpTests();TrustTests();DsmTests();
    printf("{\"checks\":%d,\"passed\":true,\"scope\":\"Actual shared C epoch/queue/bounds helpers; user-mode only, not a kernel runtime test\"}\n",checks);
    return 0;
}
