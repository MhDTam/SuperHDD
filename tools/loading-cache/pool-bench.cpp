#define NOMINMAX
#ifdef BASELINE_POOL
#include "benchmark-baseline/pool.h"
#else
#include "pool.h"
#endif
#include <filesystem>
#include <thread>
#include <chrono>
#include <cstdio>
using Clock=std::chrono::steady_clock;
int wmain(int argc,wchar_t** argv){try{
 if(argc!=2)return 2;std::filesystem::create_directories(argv[1]);auto path=(std::filesystem::path(argv[1])/L"cache.bin").wstring();
 loading::Pool pool;constexpr uint32_t blocks=2048;loading::Owner owner{1,1};pool.open(path,blocks*loading::Block);std::array<unsigned char,loading::Block> bytes{};bytes.fill(19);
 for(uint64_t i=0;i<blocks;++i)if(!pool.put({owner,77,i,7},0,bytes.data(),loading::Block))return 3;
 auto begin=Clock::now();for(unsigned i=0;i<500;++i)if(pool.summary(owner).resident!=blocks*loading::Block)return 4;
 double summaryUs=std::chrono::duration<double,std::micro>(Clock::now()-begin).count()/500;
 printf("{\"summaryAverageUs\":%.4f,\"blocks\":%u,\"cases\":[",summaryUs,blocks);bool first=true;
 for(unsigned threads:{1u,8u})for(uint32_t length:{4096u,65536u}){
  std::atomic_bool bad{false},go{false};std::atomic_uint ready{0};std::atomic_uint64_t hits{0},misses{0};std::vector<std::thread> workers;std::vector<std::vector<double>> times(threads);
  for(unsigned t=0;t<threads;++t)workers.emplace_back([&,t]{std::array<unsigned char,loading::Block> out{};++ready;while(!go)std::this_thread::yield();for(unsigned i=0;i<1024;++i){auto start=Clock::now();bool hit=pool.read({owner,77,(static_cast<uint64_t>(i)*509+t*17)%blocks,7},0,length,out.data());times[t].push_back(std::chrono::duration<double,std::micro>(Clock::now()-start).count());if(hit){++hits;for(uint32_t n=0;n<length;++n)if(out[n]!=19)bad=true;}else{++misses;Sleep(1);}}});
  while(ready<threads)std::this_thread::yield();auto start=Clock::now();go=true;for(auto& worker:workers)worker.join();double seconds=std::chrono::duration<double>(Clock::now()-start).count();if(bad)return 5;
  std::vector<double> all;for(const auto& list:times)all.insert(all.end(),list.begin(),list.end());std::sort(all.begin(),all.end());
  printf("%s{\"threads\":%u,\"requestBytes\":%u,\"hits\":%llu,\"misses\":%llu,\"hitPercent\":%.3f,\"verifiedMiBPerSecond\":%.3f,\"p95AttemptUs\":%.3f,\"maxAttemptUs\":%.3f}",first?"":",",threads,length,hits.load(),misses.load(),100.0*hits/(hits+misses),hits*static_cast<double>(length)/1048576/seconds,all[(all.size()-1)*95/100],all.back());first=false;
 }
 printf("],\"passed\":true,\"scope\":\"Synthetic resident cache under contention; each miss sleeps 1ms instead of reading HDD. Not game throughput.\"}\n");return 0;
 }catch(DWORD error){fprintf(stderr,"Win32 %lu\n",error);}catch(...){fprintf(stderr,"Pool benchmark failed\n");}return 1;}
