#define NOMINMAX
#define SUPERHDD_POOL_TEST
#include "pool.h"
#include "admission.h"
#include <filesystem>
#include <iostream>
#include <thread>
#include <atomic>
using namespace loading;
static void Check(bool value,const char* message){if(!value)throw std::runtime_error(message);}
struct Hash {size_t operator()(uint64_t value)const{return static_cast<size_t>(value);}};
static Key K(Owner owner,uint64_t block){return {owner,99,block,7};}
static void Child(const std::wstring& exe,const std::wstring& mode,const std::wstring& path){std::wstring command=L"\""+exe+L"\" "+mode+L" \""+path+L"\"";STARTUPINFOW start{};start.cb=sizeof(start);PROCESS_INFORMATION child{};Check(CreateProcessW(nullptr,command.data(),nullptr,nullptr,FALSE,CREATE_NO_WINDOW,nullptr,nullptr,&start,&child)!=FALSE,"child start");WaitForSingleObject(child.hProcess,30000);DWORD code=1;GetExitCodeProcess(child.hProcess,&code);CloseHandle(child.hThread);CloseHandle(child.hProcess);Check(code==0,"child failed");}
int wmain(int argc,wchar_t** argv){try{
 if(argc==3){Pool p;p.open(argv[2],8*Block);if(std::wstring(argv[1])==L"abandon")p.abandonForTest();else{std::array<unsigned char,Block> bytes{};bytes.fill(42);for(uint64_t i=0;i<8;++i)Check(p.put(K({2,2},i),0,bytes.data(),Block),"child fill");}return 0;}
 Check(argc==2,"test directory required");std::filesystem::create_directories(argv[1]);auto path=(std::filesystem::path(argv[1])/L"pool.bin").wstring();Owner a{1,1},b{2,2};Pool p;p.open(path,8*Block);std::array<unsigned char,Block> bytes{},out{};bytes.fill(17);
 DWORD high=0;uint64_t initial=GetCompressedFileSizeW(path.c_str(),&high)|(static_cast<uint64_t>(high)<<32);Check(initial==0,"pool eagerly allocates SSD bytes");
 for(uint64_t i=0;i<8;++i)Check(p.put(K(a,i),0,bytes.data(),Block),"fill A");Check(p.summary(a).resident==8*Block,"A cannot use entire budget");
 Check(p.read(K(a,0),0,Block,out.data()),"touch A0");bytes.fill(42);Check(p.put(K(b,0),0,bytes.data(),Block),"fill B");Check(p.read(K(a,0),0,Block,out.data()),"LRU lost recent A0");Check(!p.read(K(a,1),0,Block,out.data()),"LRU retained oldest A1");
 std::wcout<<L"PASS shared capacity and global LRU\n";
 Child(argv[0],L"fill",path);Check(p.summary(b).resident==8*Block&&p.summary(a).resident==0,"separate process did not share budget/index");
 p.invalidate(a);Check(p.read(K(b,7),0,Block,out.data())&&out[0]==42,"clear A affected B");p.resize(4*Block);Check(p.summary(b).resident==4*Block,"shrink");p.resize(8*Block);Check(p.summary(b).resident==4*Block,"grow lost residents");
 p.invalidate(b);bytes.fill(9);Check(p.put(K(a,0),1024,bytes.data(),2048),"partial insert");Check(!p.read(K(a,0),0,4096,out.data()),"partial block served an uncovered range");Check(p.read(K(a,0),1024,2048,out.data())&&out[0]==9,"partial hit");bytes.fill(8);Check(p.put(K(a,0),3072,bytes.data(),1024),"adjacent merge");Check(p.read(K(a,0),1024,3072,out.data())&&out[0]==9&&out[2048]==8,"merged bytes wrong");
 std::wcout<<L"PASS process sharing, independent clear, resize and partial ranges\n";
 p.invalidate(a);bytes.fill(10);Check(p.put(K(a,0),0,bytes.data(),Block),"corruption setup");
 // After invalidate the reused slot is implementation-dependent. Corrupt all
 // slots to prove checksum rejection without relying on index internals.
 HANDLE raw=CreateFileW(path.c_str(),GENERIC_WRITE,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_EXISTING,0,nullptr);Check(raw!=INVALID_HANDLE_VALUE,"corruption open");std::array<unsigned char,Block> zero{};for(int i=0;i<8;++i){DWORD n=0;Check(WriteFile(raw,zero.data(),Block,&n,nullptr)&&n==Block,"corruption write");}CloseHandle(raw);Check(!p.read(K(a,0),0,Block,out.data()),"corrupt cache bytes accepted");Check(p.summary(a).checksumFailures==1,"corruption counter missing");
 Child(argv[0],L"abandon",path);Check(p.summary(a).resident==0&&p.summary(b).resident==0,"abandoned lock did not reset metadata");
 std::atomic_bool bad{false};std::vector<std::thread> threads;for(uint64_t t=0;t<8;++t)threads.emplace_back([&,t]{std::array<unsigned char,Block> input{},output{};input.fill(static_cast<unsigned char>(t+1));for(uint64_t j=0;j<300;++j){auto key=K({t+10,t+10},j%16);p.put(key,0,input.data(),Block);if(p.read(key,0,Block,output.data())&&output!=input)bad=true;}});for(auto& t:threads)t.join();Check(!bad,"concurrent cache returned wrong bytes");
 std::wcout<<L"PASS checksum rejection, abandoned-lock reset and concurrent I/O\n";
 // A reader paused after metadata lookup must not hold the shared mutex.
 // Force slot reuse and abandoned-index reset before its SSD read completes.
 p.invalidate(a);p.invalidate(b);p.resize(Block);bytes.fill(17);Check(p.put(K(a,0),0,bytes.data(),Block),"race setup");
 std::atomic_bool entered{false},proceed{false};bool staleHit=true;std::array<unsigned char,Block> raced{};
 p.readBarrierForTest(&entered,&proceed);std::thread reader([&]{staleHit=p.read(K(a,0),0,Block,raced.data());});
 while(!entered)std::this_thread::yield();bytes.fill(42);Check(p.put(K(b,0),0,bytes.data(),Block),"reuse blocked by reader");proceed=true;reader.join();p.readBarrierForTest(nullptr,nullptr);
 Check(!staleHit,"reused slot returned wrong owner's bytes");Check(p.summary(a).checksumFailures==1,"legitimate reuse reported as corruption");Check(p.summary(a).readRaces>=1,"reuse race not counted");Check(p.summary(b).resident==Block,"stale reader removed new owner");
 p.invalidate(b);bytes.fill(17);Check(p.put(K(a,0),0,bytes.data(),Block),"reset race setup");entered=false;proceed=false;p.readBarrierForTest(&entered,&proceed);
 std::thread oldReader([&]{p.read(K(a,0),0,Block,raced.data());});while(!entered)std::this_thread::yield();Child(argv[0],L"abandon",path);Check(p.summary(a).resident==0,"reset did not discard old metadata");proceed=true;oldReader.join();p.readBarrierForTest(nullptr,nullptr);Check(p.summary(a).resident==0,"old reader resurrected abandoned index");
 p.resize(8*Block);bytes.fill(7);for(uint64_t i=0;i<8;++i)Check(p.put(K(a,i),0,bytes.data(),Block),"inventory setup");auto inventory=p.summary(a);Check(inventory.files.size()==1&&inventory.files[0].second==8*Block&&inventory.slots==8,"per-file accounting wrong");p.resize(4*Block);Check(p.summary(a).resident==4*Block&&p.summary(a).files[0].second==4*Block,"inventory shrink wrong");p.invalidate(a);Check(p.summary(a).files.empty(),"cleared inventory retained files");
 std::wcout<<L"PASS concurrent slot reuse, abandoned-index reader and per-file accounting\n";
 {
  // A fresh pool lifetime keeps verified records, their LRU order and their
  // file validators; it rebuilds links rather than trusting them.
  auto kept=(std::filesystem::path(argv[1])/L"persist.bin").wstring();Validator v{100,200,300},w{100,200,301};
  {Pool q;q.open(kept,4*Block);bytes.fill(51);for(uint64_t i=0;i<4;++i)Check(q.put(K(a,i),0,bytes.data(),Block,v),"persist fill");Check(q.read(K(a,0),0,Block,out.data()),"persist touch");Check(!q.put(K(a,9),0,bytes.data(),Block,w),"mismatched validator accepted");}
  {Pool q;q.open(kept,4*Block);auto s=q.summary(a);Check(s.resident==4*Block&&s.restored==4,"records lost across pool lifetime");Check(q.read(K(a,2),0,Block,out.data())&&out[0]==51,"restored bytes wrong");
   bytes.fill(52);Check(q.put(K(b,0),0,bytes.data(),Block),"post-restore fill");Check(!q.read(K(a,1),0,Block,out.data()),"rebuild lost LRU order");Check(q.read(K(a,0),0,Block,out.data()),"rebuild evicted a recent record");
   Check(q.validate(K(a,0),v),"unchanged validator rejected");Check(!q.validate(K(a,0),w),"changed source not detected");Check(q.summary(a).resident==0&&q.summary(b).resident==Block,"validator drop crossed files");}
  // Corrupt every structural link while no process holds the pool. Records
  // and their checksums alone must restore an equivalent index.
  {Pool q;q.open(kept,4*Block);bytes.fill(53);Check(q.put(K(a,5),0,bytes.data(),Block,v),"link setup");}
  {HANDLE index=CreateFileW((kept+L".index").c_str(),GENERIC_WRITE,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_EXISTING,0,nullptr);Check(index!=INVALID_HANDLE_VALUE,"index open");std::vector<unsigned char> junk(sizeof(uint32_t)*4096,0x5a);LARGE_INTEGER at{};at.QuadPart=offsetof(Header,freeHead);DWORD n=0;Check(SetFilePointerEx(index,at,nullptr,FILE_BEGIN)&&WriteFile(index,junk.data(),12,&n,nullptr),"header corrupt");at.QuadPart=sizeof(Header);Check(SetFilePointerEx(index,at,nullptr,FILE_BEGIN)&&WriteFile(index,junk.data(),static_cast<DWORD>(junk.size()),&n,nullptr),"bucket corrupt");CloseHandle(index);}
  {Pool q;q.open(kept,4*Block);Check(q.read(K(a,5),0,Block,out.data())&&out[0]==53,"rebuild did not recover corrupted links");Check(q.summary(a).resident+q.summary(b).resident<=4*Block,"rebuild over-counted residents");q.invalidate(a);q.invalidate(b);}
  // Targeted invalidation of an uncached file must leave others untouched.
  {Pool q;q.open(kept,4*Block);bytes.fill(54);Check(q.put(K(a,0),0,bytes.data(),Block),"target setup");q.invalidate(a,7,12345,false);Check(q.read(K(a,0),0,Block,out.data()),"uncached-file invalidation removed data");q.invalidate(a,7,99,false);Check(q.summary(a).resident==0,"targeted invalidation missed file");}
 }
 {
  // Concurrent adjacent partial fills merge outside the lock without ever
  // returning bytes that were not written for that range.
  p.invalidate(a);p.invalidate(b);std::atomic_bool wrong{false};std::vector<std::thread> merges;
  for(uint32_t t=0;t<8;++t)merges.emplace_back([&,t]{std::array<unsigned char,Block> in{},got{};for(uint32_t j=0;j<200;++j){uint32_t start=((t*8+j)%16)*4096;in.fill(static_cast<unsigned char>(start/4096+1));auto key=K({77,77},j%4);p.put(key,start,in.data()+start,4096);if(p.read(key,start,4096,got.data())){for(uint32_t at=0;at<4096;++at)if(got[at]!=static_cast<unsigned char>(start/4096+1)){wrong=true;break;}}}});
  for(auto& t:merges)t.join();Check(!wrong,"merged partial range returned wrong bytes");p.invalidate({77,77});
 }
 std::wcout<<L"PASS restart persistence, LRU rebuild, validators, corrupted-link recovery and concurrent merges\n";
 Admission<uint64_t,Hash> policy;policy.activity(1000,4*1024*1024);for(uint64_t i=0;i<64;++i)Check(!policy.accept(i,1000),"first touch admitted");policy.activity(1600,4*1024*1024);Check(policy.accept(0,1600),"repeated burst rejected");policy.activity(5000,4096);Check(!policy.accept(1,5000),"sparse gameplay read admitted");for(uint64_t i=100;i<70100;++i)policy.accept(i,6000);Check(policy.size()==65536,"probation is unbounded");
 std::wcout<<L"PASS first-touch rejection, burst reuse, sparse-I/O bypass and bounded probation\n";
 return 0;
 }catch(const std::exception& e){std::cerr<<e.what()<<"\n";}catch(DWORD e){std::cerr<<"Win32 "<<e<<"\n";}return 1;}
