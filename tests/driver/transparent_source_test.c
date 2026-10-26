#include <stdio.h>
#include <stdlib.h>
#include "../../drivers/superhdd-cache-driver/v7/source_policy.h"
#include "../../drivers/superhdd-cache-driver/v7/usage_state.h"
static unsigned int checks;
#define CHECK(x) do { checks++; if (!(x)) { fprintf(stderr,"Transparent policy failed at %d\n",__LINE__); exit(1); } } while(0)
int main(void) {
    SHDD_SOURCE_FACTS s={11,0,1,1,1,0,0,1,2,2,1048576,1073741824,1048576,1073741824}, f;
    SHDD_USAGE_COUNTS state={0,0,0}, next, before;
    unsigned int i,type;
    CHECK(ShddDirectSourceAllowed(&s,SHDD_DATA_ACK,0));
    for(i=0;i<1000;i++) {
        CHECK(ShddUsageNext(&state,SHDD_USAGE_PAGING,1,&next));state=next;
        CHECK(state.Paging==i+1);CHECK(ShddUsagePresent(&state));
        CHECK(!ShddUsageBlocksCache(&state));CHECK(ShddDirectSourceAllowed(&s,SHDD_DATA_ACK,0));
    }
    for(i=1000;i>0;i--){CHECK(ShddUsageNext(&state,SHDD_USAGE_PAGING,0,&next));state=next;CHECK(state.Paging==i-1);}
    CHECK(!ShddUsagePresent(&state));CHECK(!ShddUsageNext(&state,SHDD_USAGE_PAGING,0,&next));
    for(type=SHDD_USAGE_HIBERNATION;type<=SHDD_USAGE_DUMP;type++) {
        before=state;CHECK(ShddUsageNext(&state,type,1,&next));state=next;
        CHECK(ShddUsageBlocksCache(&state));f=s;f.SpecialUsage=ShddUsageBlocksCache(&state);
        CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
        state=before;CHECK(!ShddUsageBlocksCache(&state)); /* failed lower notification rollback */
        CHECK(ShddUsageNext(&state,type,1,&next));state=next;
        CHECK(ShddUsageNext(&state,type,0,&next));state=next;CHECK(!ShddUsageBlocksCache(&state));
    }
    state.Paging=0x7fffffffu;CHECK(!ShddUsageNext(&state,SHDD_USAGE_PAGING,1,&next));
    CHECK(!ShddUsageNext(&state,0,1,&next));CHECK(!ShddUsageNext(&state,4,1,&next));
    f=s;f.SystemRole=1;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    f=s;f.Removable=1;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    f=s;f.Bus=10;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    CHECK(ShddDirectSourceAllowed(&f,SHDD_VM_ACK,1));
    f=s;f.IncursSeek=0;f.Bus=17;CHECK(!ShddDirectParentAllowed(&f,2,SHDD_DATA_ACK,0));
    CHECK(ShddDirectParentAllowed(&f,3,SHDD_DATA_ACK,0));
    printf("{\"passed\":true,\"checks\":%u}\n",checks);return 0;
}
