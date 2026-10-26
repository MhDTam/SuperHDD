#define NOMINMAX
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdint>
#include <filesystem>
#include <sstream>
#include <string>
#include <thread>
#include <vector>
struct Handle {HANDLE h=INVALID_HANDLE_VALUE;explicit Handle(HANDLE x):h(x){}~Handle(){if(h!=INVALID_HANDLE_VALUE&&h)CloseHandle(h);}explicit operator bool()const{return h!=INVALID_HANDLE_VALUE&&h;}};
struct Buffer {unsigned char* p=static_cast<unsigned char*>(VirtualAlloc(nullptr,65536,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE));~Buffer(){if(p)VirtualFree(p,0,MEM_RELEASE);}};
static uint64_t word(uint64_t position){uint64_t x=position+0x9e3779b97f4a7c15ULL;x=(x^(x>>30))*0xbf58476d1ce4e5b9ULL;x=(x^(x>>27))*0x94d049bb133111ebULL;return x^(x>>31);}
struct Sample {unsigned operation=0;uint64_t offset=0;DWORD bytes=0;double ms=0;DWORD error=0;bool correct=false;};
int wmain(int argc,wchar_t** argv){
 if(argc==4&&std::wstring(argv[1])==L"make"){
  uint64_t mib=std::wcstoull(argv[3],nullptr,10);if(mib<32||mib>2048)return 2;
  Handle file(CreateFileW(argv[2],GENERIC_WRITE,0,nullptr,CREATE_NEW,FILE_FLAG_NO_BUFFERING|FILE_FLAG_WRITE_THROUGH,nullptr));Buffer buffer;if(!file||!buffer.p)return 3;
  for(uint64_t offset=0;offset<mib*1048576;offset+=65536){for(unsigned i=0;i<65536;i+=8){uint64_t x=word((offset+i)/8);memcpy(buffer.p+i,&x,8);}DWORD done=0;if(!WriteFile(file.h,buffer.p,65536,&done,nullptr)||done!=65536)return 4;}
  if(!FlushFileBuffers(file.h))return 5;printf("{\"created\":true,\"bytes\":%llu}\n",static_cast<unsigned long long>(mib*1048576));return 0;
 }
 // run FILE REQUESTS SEED THREADS mixed|train CSV
 if(argc!=8||std::wstring(argv[1])!=L"run")return 2;
 unsigned count=std::wcstoul(argv[3],nullptr,10),seed=std::wcstoul(argv[4],nullptr,10),threads=std::wcstoul(argv[5],nullptr,10);
 std::wstring kind=argv[6];bool rotated=kind==L"train-rotated";bool train=kind==L"train"||kind==L"buffered-train"||rotated;bool buffered=kind.rfind(L"buffered",0)==0;bool isSmall=kind==L"small"||kind==L"buffered-small";bool isLarge=kind==L"large"||kind==L"buffered-large";if(!train&&!isSmall&&!isLarge&&kind!=L"mixed"&&kind!=L"buffered")return 2;
 if(count<32||count>8192||threads<1||threads>8)return 2;
 Handle csv(CreateFileW(argv[7],GENERIC_WRITE,0,nullptr,CREATE_NEW,0,nullptr));
 Handle file(CreateFileW(argv[2],GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,FILE_FLAG_OVERLAPPED|(buffered?0:FILE_FLAG_NO_BUFFERING),nullptr));
 LARGE_INTEGER size{};if(!csv||!file||!GetFileSizeEx(file.h,&size)||size.QuadPart%65536)return 3;
 uint64_t blocks=static_cast<uint64_t>(size.QuadPart)/65536;if(blocks<count||(blocks&(blocks-1)))return 2;
 std::vector<Sample> samples(count);std::atomic_uint next{0},ready{0};std::atomic_bool go{false},setupFailed{false};std::vector<std::thread> workers;
 for(unsigned t=0;t<threads;++t)workers.emplace_back([&]{Buffer buffer;Handle event(CreateEventW(nullptr,TRUE,FALSE,nullptr));if(!buffer.p||!event)setupFailed=true;++ready;while(!go)std::this_thread::yield();if(!buffer.p||!event)return;
  for(;;){unsigned i=next++;if(i>=count)break;auto& s=samples[i];s.operation=i;unsigned addressIndex=rotated?(i+count/2)%count:i;s.offset=((static_cast<uint64_t>(addressIndex)*509+seed)%blocks)*65536;s.bytes=train||isLarge?65536:isSmall?4096:i%4?65536:4096;
   OVERLAPPED request{};request.Offset=static_cast<DWORD>(s.offset);request.OffsetHigh=static_cast<DWORD>(s.offset>>32);request.hEvent=event.h;ResetEvent(event.h);DWORD done=0;
   auto begin=std::chrono::steady_clock::now();BOOL ok=ReadFile(file.h,buffer.p,s.bytes,&done,&request);if(!ok&&GetLastError()==ERROR_IO_PENDING)ok=GetOverlappedResult(file.h,&request,&done,TRUE);
   s.error=ok?0:GetLastError();s.ms=std::chrono::duration<double,std::milli>(std::chrono::steady_clock::now()-begin).count();s.correct=ok&&done==s.bytes;
   if(s.correct)for(unsigned at=0;at<s.bytes;at+=8){uint64_t x=0;memcpy(&x,buffer.p+at,8);if(x!=word((s.offset+at)/8)){s.correct=false;break;}}
  }
 });
 while(ready<threads)std::this_thread::yield();auto begin=std::chrono::steady_clock::now();go=true;for(auto& thread:workers)thread.join();double seconds=std::chrono::duration<double>(std::chrono::steady_clock::now()-begin).count();
 std::vector<double> latencies;uint64_t bytes=0;unsigned errors=0;std::ostringstream lines;lines<<"operation,offset,bytes,latency_ms,error,correct\n";lines.precision(10);
 for(const auto& s:samples){latencies.push_back(s.ms);bytes+=s.bytes;if(!s.correct)++errors;lines<<s.operation<<','<<s.offset<<','<<s.bytes<<','<<s.ms<<','<<s.error<<','<<(s.correct?1:0)<<'\n';}
 auto text=lines.str();DWORD written=0;if(text.size()>MAXDWORD||!WriteFile(csv.h,text.data(),static_cast<DWORD>(text.size()),&written,nullptr)||written!=text.size())return 6;
 std::sort(latencies.begin(),latencies.end());auto percentile=[&](size_t p){return latencies[(latencies.size()-1)*p/100];};
 bool passed=!setupFailed&&errors==0;
 printf("{\"passed\":%s,\"operations\":%u,\"threads\":%u,\"seed\":%u,\"bytes\":%llu,\"seconds\":%.9f,\"MiBPerSecond\":%.6f,\"p50Ms\":%.6f,\"p95Ms\":%.6f,\"p99Ms\":%.6f,\"maxMs\":%.6f,\"errors\":%u,\"unbufferedClient\":%s}\n",passed?"true":"false",count,threads,seed,static_cast<unsigned long long>(bytes),seconds,bytes/1048576.0/seconds,percentile(50),percentile(95),percentile(99),latencies.back(),errors,buffered?"false":"true");
 return passed?0:7;
}
