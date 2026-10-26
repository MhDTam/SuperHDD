// Deterministic game-loading replay for cache comparisons. It imitates an
// Unreal-style async pak loader: several threads, each with its own handles,
// reading 64 KiB-aligned blocks (random reads mixed with sequential runs).
// The same seed always produces the same request list, so repeating a pass
// models loading the same level again. Reads are unbuffered and every byte is
// verified, so Windows' RAM cache never stands in for the storage under test.
#define NOMINMAX
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <string>
#include <thread>
#include <vector>
struct Handle {HANDLE h=INVALID_HANDLE_VALUE;Handle()=default;explicit Handle(HANDLE x):h(x){}Handle(Handle&& o)noexcept:h(o.h){o.h=INVALID_HANDLE_VALUE;}Handle(const Handle&)=delete;~Handle(){if(h!=INVALID_HANDLE_VALUE&&h)CloseHandle(h);}explicit operator bool()const{return h!=INVALID_HANDLE_VALUE&&h;}};
constexpr DWORD Chunk=65536;
static uint64_t Word(uint32_t file,uint64_t position){uint64_t x=position+0x9e3779b97f4a7c15ULL*(file+1);x=(x^(x>>30))*0xbf58476d1ce4e5b9ULL;x=(x^(x>>27))*0x94d049bb133111ebULL;return x^(x>>31);}
static std::wstring Name(const std::wstring& dir,uint32_t i){wchar_t n[32];swprintf_s(n,L"\\pak%02u.pak",i);return dir+n;}
struct Rng {uint64_t s;uint64_t next(){s^=s<<13;s^=s>>7;s^=s<<17;return s;}uint64_t below(uint64_t n){return next()%n;}};
// One work item: `count` consecutive chunks of `bytes` starting at `offset`.
struct Item {uint32_t file;uint64_t offset;uint32_t count,bytes;};
static std::vector<Item> Plan(uint32_t files,uint64_t fileBytes,uint32_t seed){
 Rng rng{0x2545F4914F6CDD1DULL^(static_cast<uint64_t>(seed)*0x9E3779B97F4A7C15ULL)};std::vector<Item> items;uint64_t chunks=fileBytes/Chunk;
 // A level touches a subset of archives; most reads are scattered blocks.
 for(int i=0;i<2400;++i){uint32_t f=static_cast<uint32_t>(rng.below(files));uint32_t bytes=rng.below(4)==0?16384:Chunk;items.push_back({f,rng.below(chunks)*Chunk,1,bytes});}
 for(int i=0;i<24;++i){uint32_t f=static_cast<uint32_t>(rng.below(files));uint32_t n=16+static_cast<uint32_t>(rng.below(49));items.push_back({f,rng.below(chunks-n)*Chunk,n,Chunk});}
 for(int i=0;i<4;++i){uint32_t f=static_cast<uint32_t>(rng.below(files));uint32_t n=256;items.push_back({f,rng.below(chunks-n)*Chunk,n,Chunk});}
 for(size_t i=items.size();i>1;--i)std::swap(items[i-1],items[rng.below(i)]);
 return items;
}
int wmain(int argc,wchar_t** argv){
 if(argc==5&&std::wstring(argv[1])==L"make"){
  std::wstring dir=argv[2];uint32_t files=std::wcstoul(argv[3],nullptr,10);uint64_t mib=std::wcstoull(argv[4],nullptr,10);if(files<1||files>64||mib<16||mib>1024)return 2;
  auto* buffer=static_cast<unsigned char*>(VirtualAlloc(nullptr,1<<20,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE));if(!buffer)return 3;
  for(uint32_t f=0;f<files;++f){Handle file(CreateFileW(Name(dir,f).c_str(),GENERIC_WRITE,0,nullptr,CREATE_NEW,FILE_FLAG_NO_BUFFERING|FILE_FLAG_WRITE_THROUGH,nullptr));if(!file)return 4;
   for(uint64_t at=0;at<mib<<20;at+=1<<20){for(uint32_t i=0;i<(1u<<20);i+=8){uint64_t x=Word(f,(at+i)/8);memcpy(buffer+i,&x,8);}DWORD done=0;if(!WriteFile(file.h,buffer,1<<20,&done,nullptr)||done!=(1u<<20))return 5;}}
  printf("{\"created\":true,\"files\":%u,\"fileBytes\":%llu}\n",files,static_cast<unsigned long long>(mib<<20));return 0;
 }
 // run DIR FILES FILE_MIB SEED THREADS
 if(argc!=7||std::wstring(argv[1])!=L"run"){fwprintf(stderr,L"Usage: loading-replay make DIR FILES MIB | run DIR FILES MIB SEED THREADS\n");return 2;}
 std::wstring dir=argv[2];uint32_t files=std::wcstoul(argv[3],nullptr,10),seed=std::wcstoul(argv[5],nullptr,10),threads=std::wcstoul(argv[6],nullptr,10);uint64_t fileBytes=std::wcstoull(argv[4],nullptr,10)<<20;
 if(files<1||files>64||threads<1||threads>16)return 2;auto items=Plan(files,fileBytes,seed);
 std::atomic_size_t next{0};std::atomic_uint64_t bytes{0},reads{0},wrong{0},failed{0};std::vector<std::vector<double>> latency(threads);std::atomic_uint ready{0};std::atomic_bool go{false};
 std::vector<std::thread> pool;
 for(uint32_t t=0;t<threads;++t)pool.emplace_back([&,t]{
  std::vector<Handle> handles;for(uint32_t f=0;f<files;++f)handles.emplace_back(CreateFileW(Name(dir,f).c_str(),GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,FILE_FLAG_NO_BUFFERING|FILE_FLAG_OVERLAPPED,nullptr));
  auto* buffer=static_cast<unsigned char*>(VirtualAlloc(nullptr,Chunk,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE));Handle event(CreateEventW(nullptr,TRUE,FALSE,nullptr));
  bool ok=buffer&&event;for(auto& h:handles)ok=ok&&static_cast<bool>(h);if(!ok)++failed;++ready;while(!go)std::this_thread::yield();
  for(size_t i;ok&&(i=next++)<items.size();){const auto& item=items[i];
   for(uint32_t c=0;c<item.count;++c){uint64_t offset=item.offset+static_cast<uint64_t>(c)*Chunk;OVERLAPPED request{};request.Offset=static_cast<DWORD>(offset);request.OffsetHigh=static_cast<DWORD>(offset>>32);request.hEvent=event.h;ResetEvent(event.h);DWORD done=0;
    auto begin=std::chrono::steady_clock::now();BOOL r=ReadFile(handles[item.file].h,buffer,item.bytes,&done,&request);if(!r&&GetLastError()==ERROR_IO_PENDING)r=GetOverlappedResult(handles[item.file].h,&request,&done,TRUE);
    latency[t].push_back(std::chrono::duration<double,std::milli>(std::chrono::steady_clock::now()-begin).count());++reads;
    if(!r||done!=item.bytes){++failed;continue;}bytes+=done;
    for(uint32_t at=0;at<done;at+=8){uint64_t x=0;memcpy(&x,buffer+at,8);if(x!=Word(item.file,(offset+at)/8)){++wrong;break;}}}
  }
  if(buffer)VirtualFree(buffer,0,MEM_RELEASE);
 });
 while(ready<threads)std::this_thread::yield();auto begin=std::chrono::steady_clock::now();go=true;for(auto& t:pool)t.join();double seconds=std::chrono::duration<double>(std::chrono::steady_clock::now()-begin).count();
 std::vector<double> all;for(auto& v:latency)all.insert(all.end(),v.begin(),v.end());std::sort(all.begin(),all.end());auto pct=[&](size_t p){return all.empty()?0.0:all[(all.size()-1)*p/100];};
 bool passed=!failed&&!wrong&&!all.empty();
 printf("{\"passed\":%s,\"seconds\":%.6f,\"bytes\":%llu,\"reads\":%llu,\"MiBPerSecond\":%.3f,\"p50Ms\":%.4f,\"p95Ms\":%.4f,\"p99Ms\":%.4f,\"maxMs\":%.4f,\"wrong\":%llu,\"failed\":%llu,\"items\":%zu,\"threads\":%u,\"seed\":%u}\n",passed?"true":"false",seconds,static_cast<unsigned long long>(bytes.load()),static_cast<unsigned long long>(reads.load()),bytes/1048576.0/seconds,pct(50),pct(95),pct(99),all.empty()?0.0:all.back(),static_cast<unsigned long long>(wrong.load()),static_cast<unsigned long long>(failed.load()),items.size(),threads,seed);
 return passed?0:7;
}
