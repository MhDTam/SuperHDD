// One owner-protected, sparse SSD pool per configuration. All mounts share its
// index and LRU. Cache failure is disposable; source files are never stored here.
#pragma once
#include <windows.h>
#include <winioctl.h>
#include <bcrypt.h>
#include <array>
#include <atomic>
#include <algorithm>
#include <cstdint>
#include <cstring>
#include <string>
#include <vector>
namespace loading {
constexpr uint32_t Block=65536,Nil=UINT32_MAX,MaxSlots=25000000000ULL/Block,Buckets=1<<20;
struct Owner {uint64_t a=0,b=0;bool operator==(const Owner& o)const{return a==o.a&&b==o.b;}};
struct Key {Owner owner;uint64_t file=0,block=0;uint32_t volume=0;bool operator==(const Key& k)const{return owner==k.owner&&file==k.file&&block==k.block&&volume==k.volume;}};
struct Record {Key key;uint32_t start=0,length=0,nextHash=Nil,previous=Nil,next=Nil;std::array<unsigned char,32> digest{};};
struct Header {uint64_t magic=0;uint32_t version=0,limit=0,nextUnused=0,freeHead=Nil,head=Nil,tail=Nil;uint64_t occupied=0,entries=0,evictions=0,epoch=0;};
struct Summary {uint64_t capacity=0,resident=0,slots=0,evictions=0,totalResident=0,checksumFailures=0,errors=0;std::vector<std::pair<Key,uint32_t>> files;};
class Pool {
 HANDLE mutex_=nullptr,data_=INVALID_HANDLE_VALUE,meta_=INVALID_HANDLE_VALUE,mapping_=nullptr;
 void* view_=nullptr;Header* h_=nullptr;uint32_t* buckets_=nullptr;Record* records_=nullptr;BCRYPT_ALG_HANDLE hash_=nullptr;
 uint32_t initialLimit_=0;std::atomic_uint64_t checksumFailures_{0},errors_{0};
 static constexpr uint64_t Magic=0x534844504f4f4c31ULL;
 struct Lock {Pool* p=nullptr;bool abandoned=false;explicit Lock(Pool* pool,DWORD wait){DWORD r=WaitForSingleObject(pool->mutex_,wait);if(r==WAIT_OBJECT_0||r==WAIT_ABANDONED){p=pool;abandoned=r==WAIT_ABANDONED;if(abandoned&&p->h_)p->reset(p->h_->limit?p->h_->limit:p->initialLimit_);}}~Lock(){if(p)ReleaseMutex(p->mutex_);}explicit operator bool()const{return p!=nullptr;}};
 static bool seek(HANDLE file,uint64_t offset){LARGE_INTEGER p;p.QuadPart=static_cast<LONGLONG>(offset);return SetFilePointerEx(file,p,nullptr,FILE_BEGIN)!=FALSE;}
 static uint32_t bucket(const Key& k){uint64_t x=k.file^(k.block*0x9e3779b97f4a7c15ULL)^k.owner.a^k.owner.b^k.volume;x^=x>>33;x*=0xff51afd7ed558ccdULL;x^=x>>33;return static_cast<uint32_t>(x)&(Buckets-1);}
 std::array<unsigned char,32> digest(const unsigned char* p,uint32_t length){std::array<unsigned char,32> out{};if(BCryptHash(hash_,nullptr,0,const_cast<PUCHAR>(p),length,out.data(),32)<0)throw DWORD(ERROR_CRC);return out;}
 void reset(uint32_t limit){uint64_t epoch=h_->epoch+1;*h_=Header{};h_->magic=Magic;h_->version=1;h_->limit=limit;h_->epoch=epoch;std::fill_n(buckets_,Buckets,Nil);}
 uint32_t find(const Key& key)const{for(uint32_t i=buckets_[bucket(key)],n=0;i!=Nil&&n++<MaxSlots;i=records_[i].nextHash){if(i>=h_->nextUnused)return Nil;if(records_[i].key==key)return i;}return Nil;}
 void unlinkLru(uint32_t i){auto& r=records_[i];if(r.previous==Nil)h_->head=r.next;else records_[r.previous].next=r.next;if(r.next==Nil)h_->tail=r.previous;else records_[r.next].previous=r.previous;}
 void touch(uint32_t i){if(h_->tail==i)return;unlinkLru(i);auto& r=records_[i];r.previous=h_->tail;r.next=Nil;if(h_->tail!=Nil)records_[h_->tail].next=i;else h_->head=i;h_->tail=i;}
 void remove(uint32_t i){auto& r=records_[i];uint32_t* p=&buckets_[bucket(r.key)];while(*p!=Nil&&*p!=i)p=&records_[*p].nextHash;if(*p==i)*p=r.nextHash;unlinkLru(i);h_->occupied-=r.length;--h_->entries;r.length=0;r.nextHash=h_->freeHead;h_->freeHead=i;}
 bool readRecord(uint32_t i,unsigned char* out){auto& r=records_[i];DWORD n=0;if(!seek(data_,static_cast<uint64_t>(i)*Block+r.start)||!ReadFile(data_,out,r.length,&n,nullptr)||n!=r.length){++errors_;return false;}if(digest(out,r.length)!=r.digest){++checksumFailures_;return false;}return true;}
public:
 Pool()=default;Pool(const Pool&)=delete;Pool& operator=(const Pool&)=delete;
 ~Pool(){if(view_)UnmapViewOfFile(view_);if(mapping_)CloseHandle(mapping_);if(meta_!=INVALID_HANDLE_VALUE)CloseHandle(meta_);if(data_!=INVALID_HANDLE_VALUE)CloseHandle(data_);if(mutex_)CloseHandle(mutex_);if(hash_)BCryptCloseAlgorithmProvider(hash_,0);}
 void open(const std::wstring& path,uint64_t capacity){
  if(capacity<Block||capacity>25000000000ULL)throw DWORD(ERROR_INVALID_PARAMETER);initialLimit_=static_cast<uint32_t>(capacity/Block);
  if(BCryptOpenAlgorithmProvider(&hash_,BCRYPT_SHA256_ALGORITHM,nullptr,0)<0)throw DWORD(ERROR_CRC);
  auto id=digest(reinterpret_cast<const unsigned char*>(path.data()),static_cast<uint32_t>(path.size()*sizeof(wchar_t)));std::wstring name=L"Local\\SuperHDD-Pool-";for(auto c:id){wchar_t part[3];swprintf_s(part,L"%02x",c);name+=part;}
  mutex_=CreateMutexW(nullptr,FALSE,name.c_str());bool fresh=GetLastError()!=ERROR_ALREADY_EXISTS;if(!mutex_)throw GetLastError();Lock lock(this,10000);if(!lock)throw DWORD(ERROR_TIMEOUT);
  data_=CreateFileW(path.c_str(),GENERIC_READ|GENERIC_WRITE,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_ALWAYS,FILE_FLAG_OPEN_REPARSE_POINT,nullptr);if(data_==INVALID_HANDLE_VALUE)throw GetLastError();
  DWORD n=0;if(!DeviceIoControl(data_,FSCTL_SET_SPARSE,nullptr,0,nullptr,0,&n,nullptr))throw GetLastError();
  meta_=CreateFileW((path+L".index").c_str(),GENERIC_READ|GENERIC_WRITE,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_ALWAYS,FILE_FLAG_OPEN_REPARSE_POINT,nullptr);if(meta_==INVALID_HANDLE_VALUE)throw GetLastError();
  for(HANDLE file:{data_,meta_}){BY_HANDLE_FILE_INFORMATION info{};if(!GetFileInformationByHandle(file,&info))throw GetLastError();if(info.dwFileAttributes&(FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_ENCRYPTED))throw DWORD(ERROR_ACCESS_DENIED);}
  uint64_t bytes=sizeof(Header)+static_cast<uint64_t>(Buckets)*sizeof(uint32_t)+static_cast<uint64_t>(MaxSlots)*sizeof(Record);
  mapping_=CreateFileMappingW(meta_,nullptr,PAGE_READWRITE,static_cast<DWORD>(bytes>>32),static_cast<DWORD>(bytes),nullptr);if(!mapping_)throw GetLastError();
  view_=MapViewOfFile(mapping_,FILE_MAP_ALL_ACCESS,0,0,static_cast<SIZE_T>(bytes));if(!view_)throw GetLastError();h_=static_cast<Header*>(view_);buckets_=reinterpret_cast<uint32_t*>(h_+1);records_=reinterpret_cast<Record*>(buckets_+Buckets);
  if(fresh||lock.abandoned||h_->magic!=Magic||h_->version!=1||!h_->limit||h_->limit>MaxSlots)reset(initialLimit_);
  if(fresh){LARGE_INTEGER old{};if(GetFileSizeEx(data_,&old)&&old.QuadPart){FILE_ZERO_DATA_INFORMATION range{};range.BeyondFinalZero=old;DeviceIoControl(data_,FSCTL_SET_ZERO_DATA,&range,sizeof(range),nullptr,0,&n,nullptr);}}
  if(!seek(data_,static_cast<uint64_t>(h_->limit)*Block)||!SetEndOfFile(data_))throw GetLastError();
 }
 // Foreground never waits for another worker's fill or metadata lock.
 bool read(const Key& key,uint32_t start,uint32_t length,unsigned char* out){Lock lock(this,0);if(!lock)return false;uint32_t i=find(key);if(i==Nil)return false;auto& r=records_[i];if(start<r.start||start+length>r.start+r.length)return false;std::array<unsigned char,Block> buffer{};if(!readRecord(i,buffer.data())){remove(i);return false;}memcpy(out,buffer.data()+start-r.start,length);touch(i);return true;}
 bool put(const Key& key,uint32_t start,const unsigned char* bytes,uint32_t length){
  if(!length||start+length>Block)return false;Lock lock(this,0);if(!lock)return false;
  uint32_t i=find(key);std::array<unsigned char,Block> merged{};
  if(i!=Nil){auto old=records_[i];if(start>=old.start&&start+length<=old.start+old.length){touch(i);return true;}
   if(start<=old.start+old.length&&old.start<=start+length&&readRecord(i,merged.data()+old.start)){
    memcpy(merged.data()+start,bytes,length);uint32_t end=std::max(start+length,old.start+old.length);start=std::min(start,old.start);length=end-start;bytes=merged.data()+start;
   }remove(i);
  }
  if(h_->freeHead!=Nil){i=h_->freeHead;h_->freeHead=records_[i].nextHash;}
  else if(h_->nextUnused<h_->limit)i=h_->nextUnused++;
  else {if(h_->head==Nil)return false;i=h_->head;remove(i);h_->freeHead=records_[i].nextHash;++h_->evictions;}
  DWORD n=0;if(!seek(data_,static_cast<uint64_t>(i)*Block+start)||!WriteFile(data_,bytes,length,&n,nullptr)||n!=length){records_[i].nextHash=h_->freeHead;h_->freeHead=i;++errors_;return false;}
  Record r;r.key=key;r.start=start;r.length=length;r.digest=digest(bytes,length);r.nextHash=buckets_[bucket(key)];r.previous=h_->tail;records_[i]=r;buckets_[bucket(key)]=i;if(h_->tail!=Nil)records_[h_->tail].next=i;else h_->head=i;h_->tail=i;h_->occupied+=length;++h_->entries;return true;
 }
 void invalidate(Owner owner,uint32_t volume=0,uint64_t file=0,bool all=true){Lock lock(this,10000);if(!lock)throw DWORD(ERROR_TIMEOUT);std::vector<uint32_t> removed;for(uint32_t i=h_->head;i!=Nil;){uint32_t next=records_[i].next;const auto& k=records_[i].key;if(k.owner==owner&&(all||(k.volume==volume&&k.file==file))){if(all)removed.push_back(i);remove(i);}i=next;}if(all){std::sort(removed.begin(),removed.end());for(size_t j=0;j<removed.size();){size_t end=j+1;while(end<removed.size()&&removed[end]==removed[end-1]+1)++end;FILE_ZERO_DATA_INFORMATION range{};range.FileOffset.QuadPart=static_cast<LONGLONG>(removed[j])*Block;range.BeyondFinalZero.QuadPart=static_cast<LONGLONG>(removed[end-1]+1)*Block;DWORD n=0;DeviceIoControl(data_,FSCTL_SET_ZERO_DATA,&range,sizeof(range),nullptr,0,&n,nullptr);j=end;}}}
 void resize(uint64_t capacity){if(capacity<Block||capacity>25000000000ULL)throw DWORD(ERROR_INVALID_PARAMETER);Lock lock(this,10000);if(!lock)throw DWORD(ERROR_TIMEOUT);uint32_t limit=static_cast<uint32_t>(capacity/Block);for(uint32_t i=h_->head;i!=Nil;){uint32_t next=records_[i].next;if(i>=limit)remove(i);i=next;}h_->nextUnused=std::min(h_->nextUnused,limit);h_->freeHead=Nil;for(uint32_t i=0;i<h_->nextUnused;++i)if(!records_[i].length){records_[i].nextHash=h_->freeHead;h_->freeHead=i;}if(!seek(data_,static_cast<uint64_t>(limit)*Block)||!SetEndOfFile(data_))throw GetLastError();h_->limit=limit;}
#ifdef SUPERHDD_POOL_TEST
 void abandonForTest(){WaitForSingleObject(mutex_,INFINITE);h_->occupied=123;ExitProcess(0);}
#endif
 Summary summary(Owner owner){Lock lock(this,10000);if(!lock)throw DWORD(ERROR_TIMEOUT);Summary s;s.capacity=static_cast<uint64_t>(h_->limit)*Block;s.evictions=h_->evictions;s.totalResident=h_->occupied;s.checksumFailures=checksumFailures_;s.errors=errors_;for(uint32_t i=h_->head;i!=Nil;i=records_[i].next){auto& r=records_[i];if(r.key.owner==owner){s.resident+=r.length;++s.slots;s.files.emplace_back(r.key,r.length);}}return s;}
};
}
