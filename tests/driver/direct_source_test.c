#include <stdio.h>
#include <stdlib.h>
#include "../../drivers/superhdd-cache-driver/v5/source_policy.h"
static unsigned int checks;
#define CHECK(x) do { checks++; if (!(x)) { fprintf(stderr,"Policy failed at %d\n",__LINE__); exit(1); } } while(0)
int main(void) {
    SHDD_SOURCE_FACTS s={11,0,1,1,1,0,0,1,2,2,1048576,1073741824,1048576,1073741824}, f;
    unsigned int bus;
    CHECK(ShddDirectSourceAllowed(&s,SHDD_DATA_ACK,0));
    for(bus=0;bus<=20;bus++) { f=s;f.Bus=bus;CHECK(ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0)==(bus==3||bus==11)); }
    f=s;f.Removable=1;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    f=s;f.SystemRole=1;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    f=s;f.SpecialUsage=1;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    f=s;f.SingleExtent=0;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    f=s;f.SerialPresent=0;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    f=s;f.SeekKnown=0;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    f=s;f.IncursSeek=0;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    f=s;f.ExpectedDisk++;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    f=s;f.ExpectedOffset++;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    f=s;f.ExpectedLength++;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    f=s;f.Offset=f.ExpectedOffset=0x7fffffffffffffffULL;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    f=s;f.Length=f.ExpectedLength=0x8000000000000000ULL;CHECK(!ShddDirectSourceAllowed(&f,SHDD_DATA_ACK,0));
    CHECK(!ShddDirectSourceAllowed(&s,SHDD_VM_ACK,0));
    CHECK(!ShddDirectSourceAllowed(&s,SHDD_DATA_ACK,1));
    f=s;f.Bus=10;f.SerialPresent=f.SeekKnown=0;CHECK(ShddDirectSourceAllowed(&f,SHDD_VM_ACK,1));
    f.SystemRole=1;CHECK(!ShddDirectSourceAllowed(&f,SHDD_VM_ACK,1));
    f=s;f.Bus=17;f.IncursSeek=0;CHECK(ShddDirectParentAllowed(&f,1,SHDD_DATA_ACK,0));
    CHECK(!ShddDirectParentAllowed(&f,2,SHDD_DATA_ACK,0));
    f.Offset=0x7fffffffffffffffULL;CHECK(!ShddDirectParentAllowed(&f,1,SHDD_DATA_ACK,0));f.Offset=s.Offset;
    f.IncursSeek=1;CHECK(!ShddDirectParentAllowed(&f,1,SHDD_DATA_ACK,0));
    f=s;f.Bus=10;CHECK(!ShddDirectParentAllowed(&f,1,SHDD_DATA_ACK,0));
    CHECK(ShddDirectParentAllowed(&f,1,SHDD_VM_ACK,1));
    printf("{\"passed\":true,\"checks\":%u}\n",checks);return 0;
}
