// Shared SSD pool, format 3: verified reads, per-file accounting and source
// validators so resident data survives worker restarts and reboots.
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
#include <utility>
#include <vector>
#include "benchmark-io.h"
namespace loading {
constexpr uint32_t Block=65536,Nil=UINT32_MAX,MaxSlots=25000000000ULL/Block,Buckets=1<<20,MaxFiles=8192,FileBuckets=16384,FormatVersion=3;
// A reserved slot is being written outside the lock; it is in no hash chain,
// LRU list or free list until it is published or released.
constexpr uint32_t Reserved=UINT32_MAX-1;
// Readers wait briefly for index metadata instead of treating a busy lock as a
// miss. Lock holders never perform SSD I/O or hashing.
constexpr DWORD ReadWaitMs=5;
struct Owner {uint64_t a=0,b=0;bool operator==(const Owner& o)const{return a==o.a&&b==o.b;}};
struct Key {Owner owner;uint64_t file=0,block=0;uint32_t volume=0;bool operator==(const Key& k)const{return owner==k.owner&&file==k.file&&block==k.block&&volume==k.volume;}};
// Source metadata captured while writers are excluded. A mismatch on a later
// lease means the file changed while it was not mounted; its records are dropped.
struct Validator {uint64_t size=0,written=0,changed=0;bool operator==(const Validator& o)const{return size==o.size&&written==o.written&&changed==o.changed;}bool operator!=(const Validator& o)const{return !(*this==o);}};
struct Record {Key key;uint32_t start=0,length=0,nextHash=Nil,previous=Nil,next=Nil;std::array<unsigned char,32> digest{};uint32_t fileIndex=Nil;uint64_t serial=0;};
struct Header {uint64_t magic=0;uint32_t version=0,limit=0,nextUnused=0,freeHead=Nil,head=Nil,tail=Nil;uint64_t occupied=0,entries=0,evictions=0,epoch=0;uint32_t fileNextUnused=0,fileFreeHead=Nil,fileHead=Nil,fileTail=Nil;uint64_t serial=0;};
struct FileRecord {Key key;uint64_t resident=0;uint32_t slots=0,nextHash=Nil,previous=Nil,next=Nil;Validator validator;};
struct Summary {uint64_t capacity=0,resident=0,slots=0,evictions=0,totalResident=0,checksumFailures=0,errors=0,readLockBypasses=0,readRaces=0,cacheReadBytes=0,restored=0;std::vector<std::pair<Key,uint64_t>> files;};
static_assert(sizeof(Header)+static_cast<uint64_t>(Buckets)*4+static_cast<uint64_t>(MaxSlots)*sizeof(Record)+FileBuckets*4+MaxFiles*sizeof(FileRecord)<64ULL*1024*1024,"Pool metadata exceeds the native free-space reserve");
class Pool {
 HANDLE mutex_=nullptr,data_=INVALID_HANDLE_VALUE,reader_=INVALID_HANDLE_VALUE,writer_=INVALID_HANDLE_VALUE,meta_=INVALID_HANDLE_VALUE,mapping_=nullptr;
 void* view_=nullptr;Header* h_=nullptr;uint32_t* buckets_=nullptr;Record* records_=nullptr;uint32_t* fileBuckets_=nullptr;FileRecord* files_=nullptr;BCRYPT_ALG_HANDLE hash_=nullptr;
 uint32_t initialLimit_=0;std::atomic_uint64_t checksumFailures_{0},errors_{0},readLockBypasses_{0},readRaces_{0},cacheReadBytes_{0},restored_{0};
#ifdef SUPERHDD_POOL_TEST
 std::atomic_bool* entered_=nullptr;std::atomic_bool* proceed_=nullptr;
#endif
 static constexpr uint64_t Magic=0x534844504f4f4c31ULL;
 struct Lock {Pool* p=nullptr;bool abandoned=false;explicit Lock(Pool* pool,DWORD wait){DWORD r=WaitForSingleObject(pool->mutex_,wait);if(r==WAIT_OBJECT_0||r==WAIT_ABANDONED){p=pool;abandoned=r==WAIT_ABANDONED;if(abandoned&&p->h_)p->reset(p->h_->limit?p->h_->limit:p->initialLimit_);}}~Lock(){if(p)ReleaseMutex(p->mutex_);}explicit operator bool()const{return p!=nullptr;}};
 struct Event {HANDLE h=CreateEventW(nullptr,TRUE,FALSE,nullptr);~Event(){if(h)CloseHandle(h);}};
 // Explicit offsets on overlapped handles: no shared file pointer, so I/O can
 // run outside the pool lock and concurrently from several threads.
 static bool transfer(HANDLE file,uint64_t offset,unsigned char* in,const unsigned char* out,uint32_t length,DWORD& done){
  thread_local Event event;if(!event.h||!ResetEvent(event.h))return false;OVERLAPPED request{};request.Offset=static_cast<DWORD>(offset);request.OffsetHigh=static_cast<DWORD>(offset>>32);request.hEvent=event.h;
  BOOL ok=in?ReadFile(file,in,length,&done,&request):WriteFile(file,out,length,&done,&request);if(ok)return true;if(GetLastError()==ERROR_IO_PENDING)return GetOverlappedResult(file,&request,&done,TRUE)!=FALSE;return false;
 }
 static uint32_t bucket(const Key& k){uint64_t x=k.file^(k.block*0x9e3779b97f4a7c15ULL)^k.owner.a^k.owner.b^k.volume;x^=x>>33;x*=0xff51afd7ed558ccdULL;x^=x>>33;return static_cast<uint32_t>(x)&(Buckets-1);}
 static Key fileKey(Key key){key.block=0;return key;}
 static uint32_t fileBucket(const Key& key){return bucket(fileKey(key))&(FileBuckets-1);}
 std::array<unsigned char,32> digest(const unsigned char* p,uint32_t length){std::array<unsigned char,32> out{};if(BCryptHash(hash_,nullptr,0,const_cast<PUCHAR>(p),length,out.data(),32)<0)throw DWORD(ERROR_CRC);return out;}
 void reset(uint32_t limit){uint64_t epoch=h_->epoch+1;*h_=Header{};h_->magic=Magic;h_->version=FormatVersion;h_->limit=limit;h_->epoch=epoch;std::fill_n(buckets_,Buckets,Nil);std::fill_n(fileBuckets_,FileBuckets,Nil);}
 uint32_t find(const Key& key)const{for(uint32_t i=buckets_[bucket(key)],n=0;i!=Nil&&n++<MaxSlots;i=records_[i].nextHash){if(i>=h_->nextUnused||i>=MaxSlots)return Nil;if(records_[i].key==key)return i;}return Nil;}
 uint32_t findFile(const Key& key)const{Key k=fileKey(key);for(uint32_t i=fileBuckets_[fileBucket(k)],n=0;i!=Nil&&n++<MaxFiles;i=files_[i].nextHash){if(i>=h_->fileNextUnused)return Nil;if(files_[i].key==k)return i;}return Nil;}
 uint32_t ensureFile(const Key& key,const Validator& validator){
  Key k=fileKey(key);uint32_t existing=findFile(k);if(existing!=Nil)return existing;uint32_t b=fileBucket(k);
  uint32_t i=Nil;if(h_->fileFreeHead!=Nil){i=h_->fileFreeHead;h_->fileFreeHead=files_[i].nextHash;}else if(h_->fileNextUnused<MaxFiles)i=h_->fileNextUnused++;if(i==Nil)return Nil;
  FileRecord f;f.key=k;f.validator=validator;f.nextHash=fileBuckets_[b];f.previous=h_->fileTail;files_[i]=f;fileBuckets_[b]=i;if(h_->fileTail!=Nil)files_[h_->fileTail].next=i;else h_->fileHead=i;h_->fileTail=i;return i;
 }
 void removeFile(uint32_t i){auto& f=files_[i];uint32_t* link=&fileBuckets_[fileBucket(f.key)];while(*link!=Nil&&*link!=i)link=&files_[*link].nextHash;if(*link==i)*link=f.nextHash;if(f.previous==Nil)h_->fileHead=f.next;else files_[f.previous].next=f.next;if(f.next==Nil)h_->fileTail=f.previous;else files_[f.next].previous=f.previous;f.slots=0;f.resident=0;f.nextHash=h_->fileFreeHead;h_->fileFreeHead=i;}
 void unlinkLru(uint32_t i){auto& r=records_[i];if(r.previous==Nil)h_->head=r.next;else records_[r.previous].next=r.next;if(r.next==Nil)h_->tail=r.previous;else records_[r.next].previous=r.previous;}
 void appendLru(uint32_t i){auto& r=records_[i];r.previous=h_->tail;r.next=Nil;if(h_->tail!=Nil)records_[h_->tail].next=i;else h_->head=i;h_->tail=i;}
 void touch(uint32_t i){if(h_->tail==i)return;unlinkLru(i);appendLru(i);}
 void release(uint32_t i){records_[i].length=0;records_[i].start=0;records_[i].nextHash=h_->freeHead;h_->freeHead=i;}
 void remove(uint32_t i){auto& r=records_[i];uint32_t* p=&buckets_[bucket(r.key)];while(*p!=Nil&&*p!=i)p=&records_[*p].nextHash;if(*p==i)*p=r.nextHash;unlinkLru(i);h_->occupied-=r.length;--h_->entries;auto& f=files_[r.fileIndex];f.resident-=r.length;if(!--f.slots)removeFile(r.fileIndex);release(i);}
 // Takes a slot off the free list, from unused space or by evicting the LRU head.
 uint32_t allocate(){uint32_t i=Nil;if(h_->freeHead!=Nil){i=h_->freeHead;h_->freeHead=records_[i].nextHash;}else if(h_->nextUnused<h_->limit)i=h_->nextUnused++;else if(h_->head!=Nil){i=h_->head;remove(i);h_->freeHead=records_[i].nextHash;++h_->evictions;}return i;}
 // Removes every record of one source file. It stops as soon as the file's
 // last slot is gone, and does nothing for files without resident data.
 void dropFile(const Key& key){uint32_t f=findFile(key);if(f==Nil)return;Key k=fileKey(key);uint32_t left=files_[f].slots;for(uint32_t i=h_->head,n=0;i!=Nil&&left&&n++<MaxSlots;){uint32_t next=records_[i].next;if(fileKey(records_[i].key)==k){remove(i);--left;}i=next;}if(left){uint32_t again=findFile(k);if(again!=Nil)removeFile(again);}}
 // Structural links are not trusted across a pool lifetime. They are rebuilt
 // from self-contained records; each record's bytes stay digest-authenticated
 // and each file's validator is rechecked before its first use.
 void rebuild(){
  uint32_t limit=h_->limit,used=std::min(h_->nextUnused,limit),fileUsed=std::min(h_->fileNextUnused,MaxFiles);uint64_t serial=h_->serial,evictions=h_->evictions;
  std::vector<std::pair<uint64_t,uint32_t>> live;std::vector<Validator> validators(fileUsed);std::vector<Key> keys(fileUsed);
  for(uint32_t f=0;f<fileUsed;++f){validators[f]=files_[f].validator;keys[f]=files_[f].key;}
  auto usable=[&](uint32_t i){const auto& r=records_[i];return r.length&&r.start<Block&&r.length<=Block-r.start&&r.fileIndex<fileUsed&&keys[r.fileIndex]==fileKey(r.key)&&files_[r.fileIndex].slots;};
  // Prefer the recorded LRU order (it includes read touches); fall back to
  // insertion order when the list itself is damaged.
  bool intact=true;std::vector<bool> listed(used,false);uint32_t previous=Nil,count=0;
  for(uint32_t i=h_->head;i!=Nil;previous=i,i=records_[i].next){if(i>=used||listed[i]||records_[i].previous!=previous||++count>used){intact=false;break;}listed[i]=true;}
  intact=intact&&previous==h_->tail&&count==h_->entries;
  if(intact){uint64_t rank=0;for(uint32_t i=h_->head;i!=Nil;i=records_[i].next)if(usable(i))live.emplace_back(++rank,i);}
  else{for(uint32_t i=0;i<used;++i)if(usable(i))live.emplace_back(records_[i].serial,i);std::sort(live.begin(),live.end());}
  std::vector<uint32_t> oldFile(used,Nil);for(const auto& item:live)oldFile[item.second]=records_[item.second].fileIndex;
  for(const auto& item:live)serial=std::max(serial,records_[item.second].serial);
  reset(limit);h_->nextUnused=used;h_->evictions=evictions;h_->serial=serial;
  std::vector<bool> keep(used,false);for(const auto& item:live)keep[item.second]=true;for(uint32_t i=used;i-->0;)if(!keep[i])release(i);
  for(const auto& item:live){uint32_t i=item.second;auto& r=records_[i];uint32_t existing=find(r.key);if(existing!=Nil)remove(existing);
   uint32_t file=ensureFile(r.key,validators[oldFile[i]]);if(file==Nil){release(i);continue;}r.fileIndex=file;r.nextHash=buckets_[bucket(r.key)];buckets_[bucket(r.key)]=i;appendLru(i);h_->occupied+=r.length;++h_->entries;files_[file].resident+=r.length;++files_[file].slots;++restored_;}
 }
 Summary summarize(Owner owner){Summary s;s.capacity=static_cast<uint64_t>(h_->limit)*Block;s.evictions=h_->evictions;s.totalResident=h_->occupied;s.checksumFailures=checksumFailures_;s.errors=errors_;s.readLockBypasses=readLockBypasses_;s.readRaces=readRaces_;s.cacheReadBytes=cacheReadBytes_;s.restored=restored_;for(uint32_t i=h_->fileHead,n=0;i!=Nil&&n++<MaxFiles;i=files_[i].next){if(i>=h_->fileNextUnused)throw DWORD(ERROR_INVALID_DATA);const auto& f=files_[i];if(f.key.owner==owner){s.resident+=f.resident;s.slots+=f.slots;s.files.emplace_back(f.key,f.resident);}}return s;}
public:
 Pool()=default;Pool(const Pool&)=delete;Pool& operator=(const Pool&)=delete;
 ~Pool(){if(view_){FlushViewOfFile(view_,0);UnmapViewOfFile(view_);}if(mapping_)CloseHandle(mapping_);if(meta_!=INVALID_HANDLE_VALUE)CloseHandle(meta_);for(HANDLE file:{reader_,writer_,data_})if(file!=INVALID_HANDLE_VALUE)CloseHandle(file);if(mutex_)CloseHandle(mutex_);if(hash_)BCryptCloseAlgorithmProvider(hash_,0);}
 void open(const std::wstring& path,uint64_t capacity){
  if(capacity<Block||capacity>25000000000ULL)throw DWORD(ERROR_INVALID_PARAMETER);initialLimit_=static_cast<uint32_t>(capacity/Block);if(BCryptOpenAlgorithmProvider(&hash_,BCRYPT_SHA256_ALGORITHM,nullptr,0)<0)throw DWORD(ERROR_CRC);
  auto id=digest(reinterpret_cast<const unsigned char*>(path.data()),static_cast<uint32_t>(path.size()*sizeof(wchar_t)));std::wstring name=L"Local\\SuperHDD-Pool-";for(auto c:id){wchar_t part[3];swprintf_s(part,L"%02x",c);name+=part;}
  mutex_=CreateMutexW(nullptr,FALSE,name.c_str());bool fresh=GetLastError()!=ERROR_ALREADY_EXISTS;if(!mutex_)throw GetLastError();Lock lock(this,10000);if(!lock)throw DWORD(ERROR_TIMEOUT);
  data_=CreateFileW(path.c_str(),GENERIC_READ|GENERIC_WRITE,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_ALWAYS,FILE_FLAG_OPEN_REPARSE_POINT|BenchmarkWriteFlags,nullptr);if(data_==INVALID_HANDLE_VALUE)throw GetLastError();DWORD n=0;if(!DeviceIoControl(data_,FSCTL_SET_SPARSE,nullptr,0,nullptr,0,&n,nullptr))throw GetLastError();
  meta_=CreateFileW((path+L".index").c_str(),GENERIC_READ|GENERIC_WRITE,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_ALWAYS,FILE_FLAG_OPEN_REPARSE_POINT,nullptr);if(meta_==INVALID_HANDLE_VALUE)throw GetLastError();
  for(HANDLE file:{data_,meta_}){BY_HANDLE_FILE_INFORMATION info{};if(!GetFileInformationByHandle(file,&info))throw GetLastError();if(info.dwFileAttributes&(FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_ENCRYPTED))throw DWORD(ERROR_ACCESS_DENIED);}
  uint64_t bytes=sizeof(Header)+static_cast<uint64_t>(Buckets)*sizeof(uint32_t)+static_cast<uint64_t>(MaxSlots)*sizeof(Record)+FileBuckets*sizeof(uint32_t)+MaxFiles*sizeof(FileRecord);
  mapping_=CreateFileMappingW(meta_,nullptr,PAGE_READWRITE,static_cast<DWORD>(bytes>>32),static_cast<DWORD>(bytes),nullptr);if(!mapping_)throw GetLastError();view_=MapViewOfFile(mapping_,FILE_MAP_ALL_ACCESS,0,0,static_cast<SIZE_T>(bytes));if(!view_)throw GetLastError();
  h_=static_cast<Header*>(view_);buckets_=reinterpret_cast<uint32_t*>(h_+1);records_=reinterpret_cast<Record*>(buckets_+Buckets);fileBuckets_=reinterpret_cast<uint32_t*>(records_+MaxSlots);files_=reinterpret_cast<FileRecord*>(fileBuckets_+FileBuckets);
  // An active older worker must never reinterpret changed index offsets.
  if(!fresh&&h_->magic==Magic&&h_->version!=FormatVersion){h_=nullptr;throw DWORD(ERROR_REVISION_MISMATCH);}
  bool consistent=h_->magic==Magic&&h_->version==FormatVersion&&h_->limit&&h_->limit<=MaxSlots&&h_->nextUnused<=h_->limit&&h_->entries<=h_->limit&&h_->fileNextUnused<=MaxFiles;
  bool discarded=lock.abandoned||!consistent;
  if(discarded)reset(initialLimit_);else if(fresh)rebuild();
  // Release the bytes of a discarded index; retained records keep their data.
  if(fresh&&discarded){LARGE_INTEGER old{};if(GetFileSizeEx(data_,&old)&&old.QuadPart){FILE_ZERO_DATA_INFORMATION range{};range.BeyondFinalZero=old;DeviceIoControl(data_,FSCTL_SET_ZERO_DATA,&range,sizeof(range),nullptr,0,&n,nullptr);}}
  if(!seek(data_,static_cast<uint64_t>(h_->limit)*Block)||!SetEndOfFile(data_))throw GetLastError();
  reader_=CreateFileW(path.c_str(),GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT|FILE_FLAG_OVERLAPPED|BenchmarkReadFlags,nullptr);if(reader_==INVALID_HANDLE_VALUE)throw GetLastError();
  writer_=CreateFileW(path.c_str(),GENERIC_WRITE,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT|FILE_FLAG_OVERLAPPED|BenchmarkWriteFlags,nullptr);if(writer_==INVALID_HANDLE_VALUE)throw GetLastError();
 }
 static bool seek(HANDLE file,uint64_t offset){LARGE_INTEGER p;p.QuadPart=static_cast<LONGLONG>(offset);return SetFilePointerEx(file,p,nullptr,FILE_BEGIN)!=FALSE;}
 // Approximate, lock-free: whether a fill can proceed without evicting.
 bool hasRoom()const{const Header* h=h_;return h&&(h->freeHead!=Nil||h->nextUnused<h->limit);}
 bool read(const Key& key,uint32_t start,uint32_t length,unsigned char* out){
  if(!length||start>=Block||length>Block-start)return false;Record record;uint32_t index=Nil;uint64_t epoch=0;
  {Lock lock(this,ReadWaitMs);if(!lock){++readLockBypasses_;return false;}index=find(key);if(index==Nil)return false;record=records_[index];epoch=h_->epoch;if(record.start>=Block||record.length>Block-record.start||start<record.start||start>record.start+record.length||length>record.start+record.length-start)return false;}
#ifdef SUPERHDD_POOL_TEST
  if(entered_){*entered_=true;while(!proceed_->load())SwitchToThread();}
#endif
  // The copied checksum authenticates the original key's bytes even if its
  // slot is concurrently reused. Mixed/torn reads never reach the caller.
  SHD_BENCH_ALIGN std::array<unsigned char,Block> buffer{};DWORD done=0;bool io=transfer(reader_,static_cast<uint64_t>(index)*Block+record.start,buffer.data(),nullptr,record.length,done)&&done==record.length;cacheReadBytes_+=done;
  bool valid=false;try{valid=io&&digest(buffer.data(),record.length)==record.digest;}catch(...){++errors_;return false;}
  {Lock lock(this,0);if(lock){auto& current=records_[index];if(epoch==h_->epoch&&index<h_->nextUnused&&current.length&&current.key==key&&current.serial==record.serial){if(valid)touch(index);else{if(io)++checksumFailures_;else ++errors_;remove(index);}}else if(!valid)++readRaces_;}else if(!valid)++readRaces_;}
  if(!valid)return false;memcpy(out,buffer.data()+start-record.start,length);return true;
 }
 bool put(const Key& key,uint32_t start,const unsigned char* bytes,uint32_t length,const Validator& validator=Validator{}){
  if(!length||start>=Block||length>Block-start)return false;Record old;uint32_t oldIndex=Nil;bool merge=false;uint64_t epoch=0;
  {Lock lock(this,1000);if(!lock)return false;epoch=h_->epoch;
   uint32_t f=findFile(key);if(f!=Nil&&files_[f].validator!=validator)return false;
   oldIndex=find(key);if(oldIndex!=Nil){old=records_[oldIndex];if(start>=old.start&&start<=old.start+old.length&&length<=old.start+old.length-start){touch(oldIndex);return true;}merge=start<=old.start+old.length&&old.start<=start+length;}}
  SHD_BENCH_ALIGN std::array<unsigned char,Block> merged{};
  if(merge){
   // Merge with the resident adjacent/overlapping range, verified outside the
   // lock. A concurrent change to that record is detected by serial below.
   DWORD done=0;merge=transfer(reader_,static_cast<uint64_t>(oldIndex)*Block+old.start,merged.data()+old.start,nullptr,old.length,done)&&done==old.length;cacheReadBytes_+=done;
   if(merge){try{merge=digest(merged.data()+old.start,old.length)==old.digest;}catch(...){++errors_;return false;}}
   if(!merge)return false;
   memcpy(merged.data()+start,bytes,length);uint32_t end=std::max(start+length,old.start+old.length);start=std::min(start,old.start);length=end-start;bytes=merged.data()+start;
  }
  auto checksum=digest(bytes,length);uint32_t slot=Nil;
  {Lock lock(this,1000);if(!lock||h_->epoch!=epoch)return false;uint32_t i=find(key);if(i!=Nil){if(merge&&records_[i].serial!=old.serial)return false;remove(i);}else if(merge)return false;
   slot=allocate();if(slot==Nil)return false;records_[slot].length=0;records_[slot].start=Reserved;records_[slot].serial=0;}
  DWORD done=0;bool written=transfer(writer_,static_cast<uint64_t>(slot)*Block+start,nullptr,bytes,length,done)&&done==length;
  Lock lock(this,10000);if(!lock)return false;
  // A reset or shrink while the slot was written already reclaimed it.
  if(h_->epoch!=epoch||slot>=h_->nextUnused||records_[slot].length||records_[slot].start!=Reserved)return false;
  if(!written){release(slot);++errors_;return false;}
  if(find(key)!=Nil){release(slot);return true;}
  uint32_t file=ensureFile(key,validator);
  // The file table is full: evict least-recent records until one file frees.
  for(uint32_t n=0;file==Nil&&h_->head!=Nil&&n<65536;++n){uint32_t victim=h_->head;remove(victim);++h_->evictions;file=ensureFile(key,validator);}
  if(file==Nil||files_[file].validator!=validator){release(slot);return false;}
  Record r;r.key=key;r.start=start;r.length=length;r.digest=checksum;r.nextHash=buckets_[bucket(key)];r.fileIndex=file;r.serial=++h_->serial;records_[slot]=r;buckets_[bucket(key)]=slot;appendLru(slot);h_->occupied+=length;++h_->entries;files_[file].resident+=length;++files_[file].slots;return true;
 }
 // Returns false and drops the file's records when its validator changed.
 bool validate(const Key& key,const Validator& validator){Lock lock(this,10000);if(!lock)throw DWORD(ERROR_TIMEOUT);uint32_t f=findFile(key);if(f==Nil||files_[f].validator==validator)return true;dropFile(key);return false;}
 void invalidate(Owner owner,uint32_t volume=0,uint64_t file=0,bool all=true){Lock lock(this,10000);if(!lock)throw DWORD(ERROR_TIMEOUT);if(!all){dropFile(Key{owner,file,0,volume});return;}std::vector<uint32_t> removed;for(uint32_t i=h_->head;i!=Nil;){uint32_t next=records_[i].next;if(records_[i].key.owner==owner){removed.push_back(i);remove(i);}i=next;}std::sort(removed.begin(),removed.end());for(size_t j=0;j<removed.size();){size_t end=j+1;while(end<removed.size()&&removed[end]==removed[end-1]+1)++end;FILE_ZERO_DATA_INFORMATION range{};range.FileOffset.QuadPart=static_cast<LONGLONG>(removed[j])*Block;range.BeyondFinalZero.QuadPart=static_cast<LONGLONG>(removed[end-1]+1)*Block;DWORD n=0;DeviceIoControl(data_,FSCTL_SET_ZERO_DATA,&range,sizeof(range),nullptr,0,&n,nullptr);j=end;}}
 void resize(uint64_t capacity){if(capacity<Block||capacity>25000000000ULL)throw DWORD(ERROR_INVALID_PARAMETER);Lock lock(this,10000);if(!lock)throw DWORD(ERROR_TIMEOUT);uint32_t limit=static_cast<uint32_t>(capacity/Block);for(uint32_t i=h_->head;i!=Nil;){uint32_t next=records_[i].next;if(i>=limit)remove(i);i=next;}h_->nextUnused=std::min(h_->nextUnused,limit);h_->freeHead=Nil;for(uint32_t i=h_->nextUnused;i-->0;)if(!records_[i].length&&records_[i].start!=Reserved){records_[i].nextHash=h_->freeHead;h_->freeHead=i;}if(!seek(data_,static_cast<uint64_t>(limit)*Block)||!SetEndOfFile(data_))throw GetLastError();h_->limit=limit;}
#ifdef SUPERHDD_POOL_TEST
 void abandonForTest(){WaitForSingleObject(mutex_,INFINITE);h_->occupied=123;ExitProcess(0);}
 void holdForTest(DWORD milliseconds){Lock lock(this,10000);Sleep(milliseconds);}
 void readBarrierForTest(std::atomic_bool* entered,std::atomic_bool* proceed){entered_=entered;proceed_=proceed;}
#endif
 Summary summary(Owner owner){Lock lock(this,10000);if(!lock)throw DWORD(ERROR_TIMEOUT);return summarize(owner);}
 bool trySummary(Owner owner,Summary& result){Lock lock(this,0);if(!lock)return false;result=summarize(owner);return true;}
};
}
