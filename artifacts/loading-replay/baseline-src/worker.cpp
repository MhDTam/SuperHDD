// Super HDD folder worker. Uses the unmodified, installed LGPL Dokan runtime.
// Each worker owns a mount and shares its configuration pool. Source writes are durable.
#define NOMINMAX
#include <dokan.h>
#include <bcrypt.h>
#include <sddl.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <memory>
#include <mutex>
#include <set>
#include <string>
#include <unordered_map>
#include <vector>
#include <shared_mutex>
#include <thread>
#include <deque>
#include <chrono>
#include <condition_variable>
#include "pool.h"
#include "admission.h"
namespace fs=std::filesystem;
constexpr DWORD Block=65536;
struct Handle {
    HANDLE h=INVALID_HANDLE_VALUE;
    Handle()=default; explicit Handle(HANDLE value):h(value){}
    ~Handle(){reset();}
    Handle(const Handle&)=delete;Handle& operator=(const Handle&)=delete;
    Handle(Handle&& other) noexcept:h(other.h){other.h=INVALID_HANDLE_VALUE;}
    Handle& operator=(Handle&& other) noexcept {if(this!=&other){reset();h=other.h;other.h=INVALID_HANDLE_VALUE;}return *this;}
    void reset(){if(h!=INVALID_HANDLE_VALUE && h!=nullptr)CloseHandle(h);h=INVALID_HANDLE_VALUE;}
    explicit operator bool()const{return h!=INVALID_HANDLE_VALUE && h!=nullptr;}
};
struct Buffer { unsigned char* p=static_cast<unsigned char*>(_aligned_malloc(Block,4096)); ~Buffer(){_aligned_free(p);} };
struct Id {DWORD volume=0;uint64_t file=0;bool operator==(const Id& o)const{return volume==o.volume && file==o.file;}};
struct Key {Id id{};uint64_t block=0;bool operator==(const Key& o)const{return id==o.id && block==o.block;}};
struct KeyHash {size_t operator()(const Key& k)const{return std::hash<uint64_t>{}(k.id.file^(k.block*0x9e3779b97f4a7c15ULL)^k.id.volume);}};
struct Lease {Handle file;std::wstring path;std::shared_mutex gate;uint64_t length=0,retryAt=0;std::atomic_uint64_t generation{0},tick{0};};
struct Context {Handle file;std::wstring path;Id id{};bool directory=false,stream=false,benchmarkClient=true;DWORD benchmarkPid=0;std::shared_ptr<Lease> lease;std::mutex io;uint64_t lastEnd=UINT64_MAX,sequential=0;};
static std::wstring BenchmarkProbePath;
static std::atomic_uint64_t BenchmarkRequested{0},BenchmarkSsd{0},BenchmarkSource{0};
static bool BenchmarkClient(DWORD pid){Handle process(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,FALSE,pid));if(!process)return false;wchar_t path[32768]{};DWORD n=32768;return QueryFullProcessImageNameW(process.h,0,path,&n)&&_wcsicmp(path,BenchmarkProbePath.c_str())==0;}
static std::mutex Mutex,PolicyMutex,QueueMutex,FillMutex;
static std::wstring Root,Mount,Cache,Ready,Stop,Status;
static Handle RootLease;
static std::unordered_map<Key,std::shared_ptr<Lease>,KeyHash> Leases;
static std::unordered_map<Context*,std::shared_ptr<Context>> Contexts;
static loading::Pool Pool;
static loading::Owner Owner;
static loading::Admission<Key,KeyHash> Policy;
struct Pending {loading::Key key;uint32_t start;std::vector<unsigned char> bytes;std::shared_ptr<Lease> lease;uint64_t generation,epoch;};
static std::deque<Pending> Queue;
static std::condition_variable QueueChanged;
static size_t QueuedBytes=0;
static std::atomic_bool Finishing{false};
static std::atomic_uint64_t Epoch{0},Hits{0},Misses{0},SsdBytes{0},SourceBytes{0},FillBytes{0},Invalidations{0},Writes{0},ChecksumFailures{0},CacheErrors{0},Tick{0},SequentialBypasses{0},AdmissionBypasses{0},QueueDrops{0};
static uint64_t Capacity=0;
static std::atomic_uint64_t ActiveLeases{0},ReadCount{0},ReadMaxUs{0};
static std::atomic_uint64_t SourceOperations{0},SourceActive{0},SourceParallelMax{0},SourceMaxUs{0};
static constexpr uint64_t LatencyBounds[]={100,250,500,1000,2500,5000,10000,25000,50000,100000,250000,UINT64_MAX};
static std::array<std::atomic_uint64_t,12> ReadLatency{};
struct ReadTimer {std::chrono::steady_clock::time_point start=std::chrono::steady_clock::now();~ReadTimer(){uint64_t us=static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::steady_clock::now()-start).count());++ReadCount;for(size_t i=0;i<12;++i)if(us<=LatencyBounds[i]){++ReadLatency[i];break;}uint64_t old=ReadMaxUs;while(us>old&&!ReadMaxUs.compare_exchange_weak(old,us)){}}};
static std::atomic_uint64_t IncomingBytes{0};
static std::atomic_bool HasResidents{false};
static BCRYPT_ALG_HANDLE Algorithm=nullptr;
static DOKAN_HANDLE Instance=nullptr;
static DOKAN_OPTIONS Options{};
static DOKAN_OPERATIONS Operations{};
static decltype(&DokanCreateFileSystem) CreateFs;
static decltype(&DokanCloseHandle) CloseFs;
static decltype(&DokanShutdown) Shutdown;
static decltype(&DokanNtStatusFromWin32) FromError;
static decltype(&DokanMapKernelToUserCreateFileFlags) MapFlags;
static decltype(&DokanOpenRequestorToken) RequestToken;
static std::atomic_bool Active{false};
static std::atomic_bool Paused{false};

static NTSTATUS Error(DWORD code=GetLastError()){fprintf(stderr,"source operation failed: Win32 %lu\n",code);fflush(stderr);return FromError(code);}
static Id Identity(HANDLE h){BY_HANDLE_FILE_INFORMATION info{};if(!GetFileInformationByHandle(h,&info))throw GetLastError();return {info.dwVolumeSerialNumber,(static_cast<uint64_t>(info.nFileIndexHigh)<<32)|info.nFileIndexLow};}
static uint64_t Size(HANDLE h){LARGE_INTEGER size{};if(!GetFileSizeEx(h,&size))throw GetLastError();return static_cast<uint64_t>(size.QuadPart);}
static bool Seek(HANDLE h,uint64_t offset){LARGE_INTEGER p{};p.QuadPart=static_cast<LONGLONG>(offset);return SetFilePointerEx(h,p,nullptr,FILE_BEGIN)!=FALSE;}
static void Maximum(std::atomic_uint64_t& target,uint64_t value){uint64_t old=target;while(value>old&&!target.compare_exchange_weak(old,value)){}}
// Explicit offsets on a leased asynchronous handle do not share a file pointer.
// A thread reuses its event only after its previous I/O has completed.
static bool ReadAt(HANDLE file,uint64_t offset,unsigned char* bytes,DWORD length,DWORD& done){
    thread_local Handle event(CreateEventW(nullptr,TRUE,FALSE,nullptr));
    if(!event)return false;
    OVERLAPPED request{};request.Offset=static_cast<DWORD>(offset);request.OffsetHigh=static_cast<DWORD>(offset>>32);request.hEvent=event.h;
    if(!ResetEvent(event.h))return false;
    if(ReadFile(file,bytes,length,&done,&request))return true;
    DWORD error=GetLastError();
    if(error==ERROR_IO_PENDING){if(GetOverlappedResult(file,&request,&done,TRUE))return true;error=GetLastError();}
    if(error==ERROR_HANDLE_EOF){done=0;return true;}SetLastError(error);return false;
}
static bool Within(const std::wstring& p){return p.size()>Root.size() && _wcsnicmp(p.c_str(),Root.c_str(),Root.size())==0 && p[Root.size()]==L'\\';}
static std::wstring Resolve(LPCWSTR name){
    std::wstring tail=name?name:L"";
    if(tail.empty() || tail[0]!=L'\\')throw DWORD(ERROR_INVALID_NAME);
    std::wstring result=Root;size_t start=1;
    while(start<tail.size()){
        size_t end=tail.find(L'\\',start);if(end==std::wstring::npos)end=tail.size();
        auto part=tail.substr(start,end-start);
        size_t colon=part.find(L':');
        if(part.empty() || part==L"." || part==L".." || part.back()==L'.' || part.back()==L' ' || part.find_first_of(L"/*?\"<>|")!=std::wstring::npos || (colon!=std::wstring::npos && (colon==0 || end!=tail.size())))throw DWORD(ERROR_INVALID_NAME);
        result+=L"\\"+part;
        auto attr=GetFileAttributesW(result.c_str());
        if(attr!=INVALID_FILE_ATTRIBUTES && (attr&FILE_ATTRIBUTE_REPARSE_POINT))throw DWORD(ERROR_ACCESS_DENIED);
        start=end+1;
    }
    return result;
}
// Hold parents against rename/reparse mutation until a native operation finishes.
static std::vector<Handle> Parents(const std::wstring& path){
    std::vector<Handle> locks;
    for(size_t end=Root.size();end<path.size();end=path.find(L'\\',end+1)){
        if(end==std::wstring::npos)break;
        auto parent=path.substr(0,end);
        Handle h(CreateFileW(parent.c_str(),FILE_READ_ATTRIBUTES,FILE_SHARE_READ,nullptr,OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,nullptr));
        if(!h)throw GetLastError();
        BY_HANDLE_FILE_INFORMATION info{};
        if(!GetFileInformationByHandle(h.h,&info))throw GetLastError();
        if(info.dwFileAttributes&FILE_ATTRIBUTE_REPARSE_POINT)throw DWORD(ERROR_ACCESS_DENIED);
        locks.emplace_back(std::move(h));
    }
    return locks;
}
static loading::Key SharedKey(Id id,uint64_t block){return {Owner,id.file,block,id.volume};}
static void Invalidate(Id id){
    auto file=Leases.find(Key{id,0});
    if(file!=Leases.end()){std::unique_lock<std::shared_mutex> lock(file->second->gate);++file->second->generation;if(file->second->file){file->second->file.reset();--ActiveLeases;}Pool.invalidate(Owner,id.volume,id.file,false);}
    else Pool.invalidate(Owner,id.volume,id.file,false);
    ++Invalidations;
}
static void InvalidatePath(const std::wstring& path){
    Handle h(CreateFileW(path.c_str(),FILE_READ_ATTRIBUTES,FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE,nullptr,OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,nullptr));
    if(h)Invalidate(Identity(h.h));
}
static void Enqueue(Pending&& item){std::lock_guard<std::mutex> lock(QueueMutex);if(QueuedBytes+item.bytes.size()>4*1024*1024){++QueueDrops;return;}QueuedBytes+=item.bytes.size();Queue.emplace_back(std::move(item));QueueChanged.notify_one();}
static void FillLoop(){
    SetThreadPriority(GetCurrentThread(),THREAD_MODE_BACKGROUND_BEGIN);
    while(!Finishing){
        Pending item;
        {std::unique_lock<std::mutex> lock(QueueMutex);QueueChanged.wait(lock,[]{return Finishing||!Queue.empty();});if(Finishing)break;item=std::move(Queue.front());QueuedBytes-=item.bytes.size();Queue.pop_front();}
        try{std::lock_guard<std::mutex> fill(FillMutex);std::shared_lock<std::shared_mutex> gate(item.lease->gate);
            if(item.epoch==Epoch&&item.generation==item.lease->generation&&!Paused){if(Pool.put(item.key,item.start,item.bytes.data(),static_cast<uint32_t>(item.bytes.size()))){FillBytes+=item.bytes.size();HasResidents=true;}else ++QueueDrops;}
        }catch(...){++CacheErrors;}
    }
    SetThreadPriority(GetCurrentThread(),THREAD_MODE_BACKGROUND_END);
}
struct FillerThread {std::thread thread{FillLoop};void stop(){Finishing=true;QueueChanged.notify_all();if(thread.joinable())thread.join();}~FillerThread(){stop();}};
static Context* Ctx(PDOKAN_FILE_INFO info){return reinterpret_cast<Context*>(info->Context);}
template<class Fn> static NTSTATUS Guard(Fn&& fn){
    try {std::lock_guard<std::mutex> lock(Mutex);return fn();}
    catch(DWORD e){return Error(e);}catch(const std::bad_alloc&){return STATUS_INSUFFICIENT_RESOURCES;}catch(...){return STATUS_INTERNAL_ERROR;}
}
struct Impersonate {
    Handle token;bool applied=false;
    explicit Impersonate(PDOKAN_FILE_INFO info):token(RequestToken(info)){
        if(token){if(!ImpersonateLoggedOnUser(token.h))throw GetLastError();applied=true;}
        else throw DWORD(ERROR_ACCESS_DENIED);
    }
    ~Impersonate(){if(applied)RevertToSelf();}
};
static NTSTATUS DOKAN_CALLBACK Open(LPCWSTR name,PDOKAN_IO_SECURITY_CONTEXT,ACCESS_MASK access,ULONG attrs,ULONG share,ULONG disposition,ULONG createOptions,PDOKAN_FILE_INFO info){
    return Guard([&]()->NTSTATUS{
        auto path=Resolve(name);auto locks=Parents(path);DWORD flags=0,creation=0;ACCESS_MASK desired=0;
        MapFlags(access,attrs,createOptions,disposition,&desired,&flags,&creation);
        auto existing=GetFileAttributesW(path.c_str());bool dir=(existing!=INVALID_FILE_ATTRIBUTES && (existing&FILE_ATTRIBUTE_DIRECTORY)) || (createOptions&FILE_DIRECTORY_FILE);
        bool modifies=(desired&(GENERIC_WRITE|GENERIC_ALL|MAXIMUM_ALLOWED|FILE_WRITE_DATA|FILE_APPEND_DATA|DELETE)) || creation!=OPEN_EXISTING;
        // A rename may open its parent with FILE_ADD_FILE. Permit directory
        // access while still refusing deletion/replacement of the mount root.
        if(path==Root && ((desired&DELETE) || creation!=OPEN_EXISTING))return STATUS_ACCESS_DENIED;
        if(modifies)InvalidatePath(path);
        if(dir){
            if((createOptions&FILE_NON_DIRECTORY_FILE)!=0)return STATUS_FILE_IS_A_DIRECTORY;
            if(existing==INVALID_FILE_ATTRIBUTES && (creation==CREATE_NEW || creation==OPEN_ALWAYS)){
                Impersonate user(info);if(!CreateDirectoryW(path.c_str(),nullptr))return Error();
            } else if(existing!=INVALID_FILE_ATTRIBUTES && creation==CREATE_NEW)return STATUS_OBJECT_NAME_COLLISION;
            else if(existing==INVALID_FILE_ATTRIBUTES)return STATUS_OBJECT_PATH_NOT_FOUND;
            flags|=FILE_FLAG_BACKUP_SEMANTICS;creation=OPEN_EXISTING;
        }
        bool benchmarkClient=true;if constexpr(loading::BenchmarkStorageIsolation)benchmarkClient=BenchmarkClient(info->ProcessId);
        flags&=~(FILE_FLAG_OVERLAPPED|FILE_FLAG_NO_BUFFERING|FILE_FLAG_DELETE_ON_CLOSE);
        flags|=FILE_FLAG_OPEN_REPARSE_POINT;
        if(!dir&&benchmarkClient)flags|=loading::BenchmarkReadFlags;
        if(modifies && !dir)flags|=FILE_FLAG_WRITE_THROUGH;
        auto context=std::make_shared<Context>();
        context->benchmarkClient=benchmarkClient;context->benchmarkPid=info->ProcessId;
        {Impersonate user(info);context->file=Handle(CreateFileW(path.c_str(),desired,share,nullptr,creation,flags,nullptr));}
        if(!context->file){auto code=GetLastError();fwprintf(stderr,L"open %ls access=0x%lx share=%lu creation=%lu flags=0x%lx: %lu\n",path.c_str(),desired,share,creation,flags,code);return Error(code);}
        BY_HANDLE_FILE_INFORMATION native{};
        if(!GetFileInformationByHandle(context->file.h,&native))return Error();
        if(native.dwFileAttributes&FILE_ATTRIBUTE_REPARSE_POINT)return STATUS_ACCESS_DENIED;
        if(native.dwFileAttributes&FILE_ATTRIBUTE_ENCRYPTED)return STATUS_NOT_SUPPORTED;
        context->path=path;context->directory=dir;context->id=Identity(context->file.h);context->stream=path.find(L':',Root.size())!=std::wstring::npos;
        auto& lease=Leases[Key{context->id,0}];if(!lease){lease=std::make_shared<Lease>();lease->path=path;}context->lease=lease;
        info->IsDirectory=dir;info->Context=reinterpret_cast<ULONG64>(context.get());Contexts.emplace(context.get(),std::move(context));
        return STATUS_SUCCESS;
    });
}
static void DOKAN_CALLBACK Cleanup(LPCWSTR,PDOKAN_FILE_INFO info){
    Guard([&]()->NTSTATUS{auto c=Ctx(info);if(c && info->DeletePending){Invalidate(c->id);FILE_DISPOSITION_INFO pending{TRUE};SetFileInformationByHandle(c->file.h,FileDispositionInfo,&pending,sizeof(pending));}return STATUS_SUCCESS;});
}
static void DOKAN_CALLBACK Close(LPCWSTR,PDOKAN_FILE_INFO info){
    Guard([&]()->NTSTATUS{auto c=Ctx(info);if(c){Contexts.erase(c);info->Context=0;}return STATUS_SUCCESS;});
}
static NTSTATUS DOKAN_CALLBACK Read(LPCWSTR,LPVOID out,DWORD requested,LPDWORD done,LONGLONG offset,PDOKAN_FILE_INFO info){
    *done=0;ReadTimer timer;
    try{
        std::shared_ptr<Context> c;
        {std::lock_guard<std::mutex> lock(Mutex);auto it=Contexts.find(Ctx(info));if(it==Contexts.end())return STATUS_INVALID_HANDLE;c=it->second;}
        if(c->directory||offset<0)return STATUS_INVALID_PARAMETER;
        auto lease=c->lease;
        std::shared_lock<std::shared_mutex> gate(lease->gate);
        if(!c->stream&&!Paused&&!lease->file&&(!loading::BenchmarkStorageIsolation||c->benchmarkClient)){
            gate.unlock();{std::unique_lock<std::shared_mutex> exclusive(lease->gate);
                if(!lease->file&&GetTickCount64()>=lease->retryAt){
                    lease->retryAt=GetTickCount64()+1000;
                    if(ActiveLeases.fetch_add(1)<512){Handle h(CreateFileW(lease->path.c_str(),GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_DELETE,nullptr,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT|FILE_FLAG_OVERLAPPED|loading::BenchmarkReadFlags,nullptr));try{if(h&&Identity(h.h)==c->id){lease->length=Size(h.h);lease->file=std::move(h);}else --ActiveLeases;}catch(...){--ActiveLeases;throw;}}else --ActiveLeases;
                }
            }gate.lock();
        }
        const bool leased=!c->stream&&!Paused&&static_cast<bool>(lease->file)&&(!loading::BenchmarkStorageIsolation||c->benchmarkClient);
        std::unique_lock<std::mutex> io(c->io,std::defer_lock);if(!leased)io.lock();
        // Denying external writers makes the leased length stable until invalidation.
        uint64_t size=leased?lease->length:Size(c->file.h),position=static_cast<uint64_t>(offset);if(position>=size||!requested)return STATUS_SUCCESS;
        DWORD count=static_cast<DWORD>(std::min<uint64_t>(requested,size-position));IncomingBytes+=count;
        const bool benchmarkRead=c->benchmarkClient&&info->ProcessId==c->benchmarkPid;
        if constexpr(loading::BenchmarkStorageIsolation)if(benchmarkRead)BenchmarkRequested+=count;
        auto source=[&](uint64_t p,unsigned char* dst,DWORD n,DWORD& got){
            auto* target=dst;
            if constexpr(loading::BenchmarkStorageIsolation){if(c->benchmarkClient){
                // Benchmark clients issue only aligned 4/64 KiB requests.
                if(p%4096||n%4096||n>Block)throw DWORD(ERROR_INVALID_PARAMETER);
                thread_local Buffer scratch;if(!scratch.p)throw DWORD(ERROR_NOT_ENOUGH_MEMORY);target=scratch.p;
            }}
            const auto start=std::chrono::steady_clock::now();++SourceOperations;Maximum(SourceParallelMax,++SourceActive);
            bool ok=leased?ReadAt(lease->file.h,p,target,n,got):(Seek(c->file.h,p)&&ReadFile(c->file.h,target,n,&got,nullptr));
            DWORD error=ok?ERROR_SUCCESS:GetLastError();--SourceActive;Maximum(SourceMaxUs,static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::steady_clock::now()-start).count()));
            if(!ok)throw error;SourceBytes+=got;
            if constexpr(loading::BenchmarkStorageIsolation)if(c->benchmarkClient){memcpy(dst,target,got);if(benchmarkRead)BenchmarkSource+=got;}
        };
        auto bytes=static_cast<unsigned char*>(out);
        if(!leased){source(position,bytes,count,*done);if(*done){uint64_t blocks=(position%Block+*done+Block-1)/Block;Misses+=blocks;AdmissionBypasses+=blocks;}return STATUS_SUCCESS;}
        lease->tick=++Tick;
        uint64_t sequential=0;{std::lock_guard<std::mutex> sequence(c->io);c->sequential=c->lastEnd==position?c->sequential+count:count;c->lastEnd=position+count;sequential=c->sequential;}
        struct Segment {DWORD at,length;uint64_t block;DWORD within;bool admit=false,hit=false;};std::vector<Segment> parts;
        uint64_t now=GetTickCount64();
        {std::lock_guard<std::mutex> policy(PolicyMutex);Policy.activity(now,count);for(DWORD at=0;at<count;){uint64_t p=position+at;DWORD within=static_cast<DWORD>(p%Block),n=std::min(Block-within,count-at);bool admit=Policy.accept(Key{c->id,p/Block},now)&&sequential<=8ULL*1024*1024;parts.push_back({at,n,p/Block,within,admit,false});at+=n;}}
        for(auto& part:parts){part.hit=HasResidents&&Pool.read(SharedKey(c->id,part.block),part.within,part.length,bytes+part.at);if(part.hit){++Hits;SsdBytes+=part.length;if constexpr(loading::BenchmarkStorageIsolation)if(benchmarkRead)BenchmarkSsd+=part.length;}else{++Misses;if(!part.admit)++AdmissionBypasses;}}
        // Batch adjacent misses into the original buffered source request. No
        // aligned 64 KiB source rereads and no SSD writes on the callback path.
        for(size_t i=0;i<parts.size();){if(parts[i].hit){++i;continue;}size_t end=i+1;while(end<parts.size()&&!parts[end].hit)++end;DWORD at=parts[i].at,n=parts[end-1].at+parts[end-1].length-at,got=0;source(position+at,bytes+at,n,got);if(got!=n){*done=at+got;return STATUS_SUCCESS;}i=end;}
        *done=count;uint64_t generation=lease->generation,epoch=Epoch;
        for(const auto& part:parts)if(!part.hit&&part.admit){Pending item{SharedKey(c->id,part.block),part.within,std::vector<unsigned char>(bytes+part.at,bytes+part.at+part.length),lease,generation,epoch};Enqueue(std::move(item));}
        if(sequential>8ULL*1024*1024)SequentialBypasses+=parts.size();
        return STATUS_SUCCESS;
    }catch(DWORD error){return Error(error);}catch(const std::bad_alloc&){return STATUS_INSUFFICIENT_RESOURCES;}catch(...){return STATUS_INTERNAL_ERROR;}
}
static NTSTATUS DOKAN_CALLBACK Write(LPCWSTR,LPCVOID bytes,DWORD length,LPDWORD done,LONGLONG offset,PDOKAN_FILE_INFO info){
    *done=0;return Guard([&]()->NTSTATUS{
        auto c=Ctx(info);if(!c || c->directory)return STATUS_INVALID_PARAMETER;
        Invalidate(c->id);std::lock_guard<std::mutex> io(c->io);uint64_t size=Size(c->file.h);
        if(info->WriteToEndOfFile)offset=static_cast<LONGLONG>(size);
        if(offset<0)return STATUS_INVALID_PARAMETER;
        if(info->PagingIo){if(static_cast<uint64_t>(offset)>=size)return STATUS_SUCCESS;length=static_cast<DWORD>(std::min<uint64_t>(length,size-static_cast<uint64_t>(offset)));}
        if(!Seek(c->file.h,static_cast<uint64_t>(offset)) || !WriteFile(c->file.h,bytes,length,done,nullptr) || !FlushFileBuffers(c->file.h))return Error();
        ++Writes;return STATUS_SUCCESS;
    });
}
static NTSTATUS DOKAN_CALLBACK Flush(LPCWSTR,PDOKAN_FILE_INFO info){return Guard([&]()->NTSTATUS{auto c=Ctx(info);return c && FlushFileBuffers(c->file.h)?STATUS_SUCCESS:Error();});}
static NTSTATUS DOKAN_CALLBACK Info(LPCWSTR name,LPBY_HANDLE_FILE_INFORMATION out,PDOKAN_FILE_INFO info){
    return Guard([&]()->NTSTATUS{auto c=Ctx(info);if(c)return GetFileInformationByHandle(c->file.h,out)?STATUS_SUCCESS:Error();
        auto path=Resolve(name);Handle h(CreateFileW(path.c_str(),FILE_READ_ATTRIBUTES,FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE,nullptr,OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,nullptr));
        return h && GetFileInformationByHandle(h.h,out)?STATUS_SUCCESS:Error();});
}
static NTSTATUS DOKAN_CALLBACK Find(LPCWSTR name,PFillFindData fill,PDOKAN_FILE_INFO info){
    return Guard([&]()->NTSTATUS{auto path=Resolve(name);auto locks=Parents(path+L"\\_");WIN32_FIND_DATAW row{};
        HANDLE search=FindFirstFileW((path+L"\\*").c_str(),&row);if(search==INVALID_HANDLE_VALUE)return Error();
        do{if(fill(&row,info)!=0)break;}while(FindNextFileW(search,&row));FindClose(search);return STATUS_SUCCESS;});
}
static NTSTATUS DOKAN_CALLBACK Streams(LPCWSTR name,PFillFindStreamData fill,PVOID context,PDOKAN_FILE_INFO){
    return Guard([&]()->NTSTATUS{auto path=Resolve(name);WIN32_FIND_STREAM_DATA row{};HANDLE search=FindFirstStreamW(path.c_str(),FindStreamInfoStandard,&row,0);
        if(search==INVALID_HANDLE_VALUE){auto error=GetLastError();return error==ERROR_HANDLE_EOF?STATUS_SUCCESS:Error(error);}
        do{if(!fill(&row,context))break;}while(FindNextStreamW(search,&row));FindClose(search);return STATUS_SUCCESS;});
}
static NTSTATUS DOKAN_CALLBACK Eof(LPCWSTR,LONGLONG length,PDOKAN_FILE_INFO info){
    return Guard([&]()->NTSTATUS{auto c=Ctx(info);if(!c || c->directory || length<0)return STATUS_INVALID_PARAMETER;Invalidate(c->id);std::lock_guard<std::mutex> io(c->io);
        if(!Seek(c->file.h,static_cast<uint64_t>(length)) || !SetEndOfFile(c->file.h) || !FlushFileBuffers(c->file.h))return Error();return STATUS_SUCCESS;});
}
static NTSTATUS DOKAN_CALLBACK Allocation(LPCWSTR name,LONGLONG length,PDOKAN_FILE_INFO info){
    if(length<0)return STATUS_INVALID_PARAMETER;
    return Guard([&]()->NTSTATUS{auto c=Ctx(info);if(!c)return STATUS_INVALID_HANDLE;
        if(static_cast<uint64_t>(length)>=Size(c->file.h))return STATUS_SUCCESS;
        Invalidate(c->id);std::lock_guard<std::mutex> io(c->io);if(!Seek(c->file.h,static_cast<uint64_t>(length)) || !SetEndOfFile(c->file.h) || !FlushFileBuffers(c->file.h))return Error();(void)name;return STATUS_SUCCESS;});
}
static NTSTATUS DOKAN_CALLBACK Delete(LPCWSTR,PDOKAN_FILE_INFO info){
    return Guard([&]()->NTSTATUS{auto c=Ctx(info);if(!c || c->path==Root)return STATUS_ACCESS_DENIED;Invalidate(c->id);
        FILE_DISPOSITION_INFO pending{static_cast<BOOLEAN>(info->DeletePending)};
        return SetFileInformationByHandle(c->file.h,FileDispositionInfo,&pending,sizeof(pending))?STATUS_SUCCESS:Error();});
}
static NTSTATUS DOKAN_CALLBACK Move(LPCWSTR,LPCWSTR target,BOOL replace,PDOKAN_FILE_INFO info){
    return Guard([&]()->NTSTATUS{auto c=Ctx(info);if(!c || c->path==Root)return STATUS_ACCESS_DENIED;auto path=Resolve(target);auto locks=Parents(path);
        Invalidate(c->id);InvalidatePath(path);size_t bytes=path.size()*sizeof(wchar_t);std::vector<unsigned char> data(offsetof(FILE_RENAME_INFO,FileName)+bytes+sizeof(wchar_t),0);
        auto rename=reinterpret_cast<FILE_RENAME_INFO*>(data.data());rename->ReplaceIfExists=static_cast<BOOLEAN>(replace);rename->RootDirectory=nullptr;rename->FileNameLength=static_cast<DWORD>(bytes);memcpy(rename->FileName,path.data(),bytes);
        if(!SetFileInformationByHandle(c->file.h,FileRenameInfo,rename,static_cast<DWORD>(data.size()))){auto code=GetLastError();fwprintf(stderr,L"rename %ls -> %ls: %lu\n",c->path.c_str(),path.c_str(),code);return Error(code);}
        const auto old=c->path;for(auto& item:Contexts){auto context=item.second;if(_wcsicmp(context->path.c_str(),old.c_str())==0 || (context->path.size()>old.size() && _wcsnicmp(context->path.c_str(),old.c_str(),old.size())==0 && context->path[old.size()]==L'\\'))context->path=path+context->path.substr(old.size());}
        for(auto& item:Leases){std::unique_lock<std::shared_mutex> gate(item.second->gate);auto& current=item.second->path;if(_wcsicmp(current.c_str(),old.c_str())==0||(current.size()>old.size()&&_wcsnicmp(current.c_str(),old.c_str(),old.size())==0&&current[old.size()]==L'\\'))current=path+current.substr(old.size());}
        return STATUS_SUCCESS;});
}
static NTSTATUS DOKAN_CALLBACK Attributes(LPCWSTR name,DWORD attrs,PDOKAN_FILE_INFO info){return Guard([&]()->NTSTATUS{auto path=Resolve(name);Impersonate user(info);return SetFileAttributesW(path.c_str(),attrs)?STATUS_SUCCESS:Error();});}
static NTSTATUS DOKAN_CALLBACK Times(LPCWSTR,const FILETIME* created,const FILETIME* accessed,const FILETIME* written,PDOKAN_FILE_INFO info){return Guard([&]()->NTSTATUS{auto c=Ctx(info);return c && SetFileTime(c->file.h,created,accessed,written)?STATUS_SUCCESS:Error();});}
static NTSTATUS DOKAN_CALLBACK Security(LPCWSTR name,PSECURITY_INFORMATION security,PSECURITY_DESCRIPTOR out,ULONG capacity,PULONG needed,PDOKAN_FILE_INFO info){
    return Guard([&]()->NTSTATUS{auto path=Resolve(name);Impersonate user(info);return GetFileSecurityW(path.c_str(),*security,out,capacity,needed)?STATUS_SUCCESS:Error();});
}
static NTSTATUS DOKAN_CALLBACK SetSecurity(LPCWSTR name,PSECURITY_INFORMATION security,PSECURITY_DESCRIPTOR descriptor,ULONG,PDOKAN_FILE_INFO info){
    return Guard([&]()->NTSTATUS{auto path=Resolve(name);Impersonate user(info);return SetFileSecurityW(path.c_str(),*security,descriptor)?STATUS_SUCCESS:Error();});
}
static NTSTATUS DOKAN_CALLBACK Space(PULONGLONG available,PULONGLONG total,PULONGLONG free,PDOKAN_FILE_INFO){return GetDiskFreeSpaceExW(Root.c_str(),reinterpret_cast<PULARGE_INTEGER>(available),reinterpret_cast<PULARGE_INTEGER>(total),reinterpret_cast<PULARGE_INTEGER>(free))?STATUS_SUCCESS:Error();}
static NTSTATUS DOKAN_CALLBACK Volume(LPWSTR label,DWORD labelCount,LPDWORD serial,LPDWORD maxPart,LPDWORD flags,LPWSTR filesystem,DWORD fsCount,PDOKAN_FILE_INFO){
    if(wcscpy_s(label,labelCount,L"Super HDD") || wcscpy_s(filesystem,fsCount,L"NTFS"))return STATUS_BUFFER_TOO_SMALL;
    *serial=0x53484446;*maxPart=255;*flags=FILE_CASE_PRESERVED_NAMES|FILE_UNICODE_ON_DISK|FILE_PERSISTENT_ACLS|FILE_NAMED_STREAMS;return STATUS_SUCCESS;
}
static NTSTATUS DOKAN_CALLBACK Mounted(LPCWSTR,PDOKAN_FILE_INFO){Active=true;return STATUS_SUCCESS;}
static NTSTATUS DOKAN_CALLBACK Unmounted(PDOKAN_FILE_INFO){Active=false;return STATUS_SUCCESS;}
static std::string Utf8(const std::wstring& value){int n=WideCharToMultiByte(CP_UTF8,0,value.c_str(),static_cast<int>(value.size()),nullptr,0,nullptr,nullptr);std::string out(static_cast<size_t>(n),0);WideCharToMultiByte(CP_UTF8,0,value.c_str(),static_cast<int>(value.size()),out.data(),n,nullptr,nullptr);return out;}
static std::string Json(const std::wstring& value){std::string out="\"";for(unsigned char c:Utf8(value)){if(c=='\\' || c=='\"'){out+='\\';out+=static_cast<char>(c);}else if(c<32){char escape[8];sprintf_s(escape,"\\u%04x",c);out+=escape;}else out+=static_cast<char>(c);}return out+'\"';}
static void Snapshot(){
    loading::Summary summary;if(!Pool.trySummary(Owner,summary))return;Capacity=summary.capacity;
    uint64_t sampleCount=0,accumulated=0,p95=0;for(auto& bucket:ReadLatency)sampleCount+=bucket.load();for(size_t i=0;i<12&&sampleCount;++i){accumulated+=ReadLatency[i].load();if(accumulated>=(sampleCount*95+99)/100){p95=LatencyBounds[i]==UINT64_MAX?ReadMaxUs.load():LatencyBounds[i];break;}}
    std::unordered_map<Key,uint64_t,KeyHash> files;for(const auto& item:summary.files)files[Key{{item.first.volume,item.first.file},0}]+=item.second;
    std::vector<std::pair<std::wstring,uint64_t>> rows;
    {std::lock_guard<std::mutex> lock(Mutex);for(const auto& item:files){auto lease=Leases.find(item.first);if(lease!=Leases.end()&&Within(lease->second->path))rows.emplace_back(Mount+lease->second->path.substr(Root.size()),item.second);}}
    auto tmp=Status+L".tmp";std::ofstream stream(fs::path(tmp),std::ios::binary|std::ios::trunc);
    stream<<"{\"active\":"<<(Active?"true":"false")<<",\"paused\":"<<(Paused?"true":"false")<<",\"backend\":\"dokanFolder\",\"cachePolicy\":\"loadingPriority\",\"sharedPool\":true,\"capacityBytes\":"<<Capacity<<",\"occupiedBytes\":"<<summary.resident<<",\"poolOccupiedBytes\":"<<summary.totalResident<<",\"sampledAt\":"<<GetTickCount64()<<",\"occupiedSlotBytes\":"<<summary.slots*Block<<",\"hits\":"<<Hits<<",\"misses\":"<<Misses<<",\"sourceReadBytes\":"<<SourceBytes<<",\"requestedReadBytes\":"<<IncomingBytes<<",\"ssdReadBytes\":"<<SsdBytes<<",\"fillBytes\":"<<FillBytes<<",\"invalidations\":"<<Invalidations<<",\"evictions\":"<<summary.evictions<<",\"sequentialBypasses\":"<<SequentialBypasses<<",\"admissionBypasses\":"<<AdmissionBypasses<<",\"queueDrops\":"<<QueueDrops<<",\"durableWrites\":"<<Writes<<",\"checksumFailures\":"<<summary.checksumFailures<<",\"cacheErrors\":"<<(CacheErrors+summary.errors)<<",\"leasedFileCount\":"<<ActiveLeases<<",\"readCount\":"<<ReadCount<<",\"readP95Us\":"<<p95<<",\"readMaxUs\":"<<ReadMaxUs<<",\"cacheLockBypasses\":"<<summary.readLockBypasses<<",\"cacheReadRaces\":"<<summary.readRaces<<",\"cacheReadApiBytes\":"<<summary.cacheReadBytes<<",\"poolFormatVersion\":2,\"sourceReadOperations\":"<<SourceOperations<<",\"sourceParallelMax\":"<<SourceParallelMax<<",\"sourceMaxUs\":"<<SourceMaxUs<<",\"dedicatedRamCacheBytes\":0,\"mount\":"<<Json(Mount)<<",\"cachedFiles\":[";
    bool first=true;for(const auto& row:rows){if(!first)stream<<',';first=false;stream<<"{\"path\":"<<Json(row.first)<<",\"residentBytes\":"<<row.second<<"}";}
    stream<<"],\"benchmarkRequestedReadBytes\":"<<BenchmarkRequested<<",\"benchmarkSsdReadBytes\":"<<BenchmarkSsd<<",\"benchmarkSourceReadBytes\":"<<BenchmarkSource<<"}";stream.close();MoveFileExW(tmp.c_str(),Status.c_str(),MOVEFILE_REPLACE_EXISTING);
}
static void Clear(){std::lock_guard<std::mutex> fill(FillMutex);++Epoch;{std::lock_guard<std::mutex> queue(QueueMutex);Queue.clear();QueuedBytes=0;}{std::lock_guard<std::mutex> metadata(Mutex);for(auto& item:Leases){std::unique_lock<std::shared_mutex> gate(item.second->gate);++item.second->generation;if(item.second->file){item.second->file.reset();--ActiveLeases;}}}Pool.invalidate(Owner);HasResidents=false;{std::lock_guard<std::mutex> policy(PolicyMutex);Policy.clear();}}
static void Resize(uint64_t capacity){Pool.resize(capacity);Capacity=capacity-capacity%Block;}
static void OwnerDescriptor(){
    Handle token;HANDLE raw=nullptr;if(!OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&raw))throw GetLastError();token=Handle(raw);
    DWORD size=0;GetTokenInformation(token.h,TokenUser,nullptr,0,&size);std::vector<unsigned char> data(size);
    if(!GetTokenInformation(token.h,TokenUser,data.data(),size,&size))throw GetLastError();LPWSTR sid=nullptr;
    if(!ConvertSidToStringSidW(reinterpret_cast<TOKEN_USER*>(data.data())->User.Sid,&sid))throw GetLastError();
    std::wstring sddl=L"D:P(A;;FA;;;SY)(A;;FA;;;"+std::wstring(sid)+L")";LocalFree(sid);PSECURITY_DESCRIPTOR descriptor=nullptr;
    if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.c_str(),SDDL_REVISION_1,&descriptor,&size))throw GetLastError();
    if(size>sizeof(Options.VolumeSecurityDescriptor)){LocalFree(descriptor);throw DWORD(ERROR_INSUFFICIENT_BUFFER);}
    memcpy(Options.VolumeSecurityDescriptor,descriptor,size);Options.VolumeSecurityDescriptorLength=size;LocalFree(descriptor);
}
int wmain(int argc,wchar_t** argv){
    if(argc==5&&!wcscmp(argv[1],L"clear-owner")){try{std::wstring owner=argv[4];if(owner.size()!=32)return 2;loading::Owner id{std::stoull(owner.substr(0,16),nullptr,16),std::stoull(owner.substr(16),nullptr,16)};loading::Pool pool;pool.open(argv[2],std::stoull(argv[3]));pool.invalidate(id);return 0;}catch(...){return 1;}}

    if(argc!=10 || wcscmp(argv[1],L"run")){fwprintf(stderr,L"Usage: folder-worker run BACKING MOUNT CACHE CAPACITY READY STOP STATUS OWNER\n");return 2;}
    try {
        Handle benchmarkParent;
        if constexpr(loading::BenchmarkStorageIsolation){
            wchar_t parent[32]{};if(!GetEnvironmentVariableW(L"SUPERHDD_BENCH_PARENT_PID",parent,32))throw DWORD(ERROR_INVALID_PARAMETER);
            benchmarkParent=Handle(OpenProcess(SYNCHRONIZE,FALSE,std::wcstoul(parent,nullptr,10)));if(!benchmarkParent)throw GetLastError();
        }
        Root=fs::canonical(argv[2]).wstring();Mount=fs::canonical(argv[3]).wstring();Cache=fs::absolute(argv[4]).wstring();Capacity=std::stoull(argv[5]);Ready=argv[6];Stop=argv[7];Status=argv[8];std::wstring owner=argv[9];if(owner.size()!=32)throw DWORD(ERROR_INVALID_PARAMETER);Owner.a=std::stoull(owner.substr(0,16),nullptr,16);Owner.b=std::stoull(owner.substr(16),nullptr,16);
        BenchmarkProbePath=(fs::path(Status).parent_path()/L"bin"/L"probe.exe").wstring();
        if(Root.size()<4 || Mount.size()<4 || (Root[1]!=L':' || Root[2]!=L'\\' || Mount[1]!=L':' || Mount[2]!=L'\\' || _wcsnicmp(Root.c_str(),Mount.c_str(),3)!=0) || Within(Mount) || _wcsicmp(Root.c_str(),Mount.c_str())==0 || Cache.size()<4 || (Cache[1]!=L':' || Cache[2]!=L'\\' || _wcsnicmp(Cache.c_str(),Root.c_str(),3)==0) || _wcsnicmp(Cache.c_str(),Root.c_str(),Root.size())==0 || Capacity<Block || Capacity>25000000000ULL || !fs::is_empty(Mount))throw DWORD(ERROR_INVALID_PARAMETER);
        for(const auto& path:{Root,Mount}){auto attr=GetFileAttributesW(path.c_str());if(attr==INVALID_FILE_ATTRIBUTES || (attr&FILE_ATTRIBUTE_REPARSE_POINT) || !(attr&FILE_ATTRIBUTE_DIRECTORY))throw DWORD(ERROR_ACCESS_DENIED);}
        Capacity-=Capacity%Block;
        RootLease=Handle(CreateFileW(Root.c_str(),FILE_READ_ATTRIBUTES,FILE_SHARE_READ,nullptr,OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,nullptr));if(!RootLease)throw GetLastError();
        Pool.open(Cache,Capacity);Pool.resize(Capacity);Pool.invalidate(Owner);
        if(BCryptOpenAlgorithmProvider(&Algorithm,BCRYPT_SHA256_ALGORITHM,nullptr,0)<0)throw DWORD(ERROR_CRC);
        HMODULE library=LoadLibraryExW(L"dokan2.dll",nullptr,LOAD_LIBRARY_SEARCH_SYSTEM32);if(!library)throw GetLastError();
        HMODULE pinned=nullptr;if(!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN|GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,reinterpret_cast<LPCWSTR>(library),&pinned))throw GetLastError();
        auto load=[&](const char* name){auto fn=GetProcAddress(library,name);if(!fn)throw DWORD(ERROR_PROC_NOT_FOUND);return fn;};
        auto init=reinterpret_cast<decltype(&DokanInit)>(load("DokanInit"));Shutdown=reinterpret_cast<decltype(Shutdown)>(load("DokanShutdown"));CreateFs=reinterpret_cast<decltype(CreateFs)>(load("DokanCreateFileSystem"));CloseFs=reinterpret_cast<decltype(CloseFs)>(load("DokanCloseHandle"));FromError=reinterpret_cast<decltype(FromError)>(load("DokanNtStatusFromWin32"));MapFlags=reinterpret_cast<decltype(MapFlags)>(load("DokanMapKernelToUserCreateFileFlags"));RequestToken=reinterpret_cast<decltype(RequestToken)>(load("DokanOpenRequestorToken"));
        auto version=reinterpret_cast<decltype(&DokanVersion)>(load("DokanVersion"));auto driverVersion=reinterpret_cast<decltype(&DokanDriverVersion)>(load("DokanDriverVersion"));
        if(version()!=DOKAN_VERSION || driverVersion()!=DOKAN_DRIVER_VERSION)throw DWORD(ERROR_REVISION_MISMATCH);
        init();Options.Version=DOKAN_VERSION;Options.Options=DOKAN_OPTION_CURRENT_SESSION|DOKAN_OPTION_ALT_STREAM;Options.MountPoint=Mount.c_str();Options.Timeout=30000;Options.AllocationUnitSize=4096;Options.SectorSize=512;OwnerDescriptor();
        Operations.ZwCreateFile=Open;Operations.Cleanup=Cleanup;Operations.CloseFile=Close;Operations.ReadFile=Read;Operations.WriteFile=Write;Operations.FlushFileBuffers=Flush;Operations.GetFileInformation=Info;Operations.FindFiles=Find;Operations.SetFileAttributes=Attributes;Operations.SetFileTime=Times;Operations.DeleteFile=Delete;Operations.DeleteDirectory=Delete;Operations.MoveFile=Move;Operations.SetEndOfFile=Eof;Operations.SetAllocationSize=Allocation;Operations.GetDiskFreeSpace=Space;Operations.GetVolumeInformation=Volume;Operations.GetFileSecurity=Security;Operations.SetFileSecurity=SetSecurity;Operations.Mounted=Mounted;Operations.Unmounted=Unmounted;
        Operations.FindStreams=Streams;
        int result=CreateFs(&Options,&Operations,&Instance);if(result!=DOKAN_SUCCESS){fwprintf(stderr,L"Dokan create failed: %d\n",result);throw DWORD(ERROR_GEN_FAILURE);}
        if constexpr(loading::BenchmarkStorageIsolation)std::ofstream(fs::path(Status+L".isolation"))<<"source-and-cache-unbuffered-v1";
        Active=true;FillerThread filler;Snapshot();std::ofstream(fs::path(Ready))<<"ready";
        while(Active && GetFileAttributesW(Stop.c_str())==INVALID_FILE_ATTRIBUTES && (!loading::BenchmarkStorageIsolation || WaitForSingleObject(benchmarkParent.h,0)==WAIT_TIMEOUT)){
            if(GetFileAttributesW((Stop+L".resize").c_str())!=INVALID_FILE_ATTRIBUTES){
                try{std::ifstream input(fs::path(Stop+L".resize"));std::string value;std::getline(input,value);Resize(std::stoull(value));}
                catch(...){std::lock_guard<std::mutex> lock(Mutex);++CacheErrors;}
                DeleteFileW((Stop+L".resize").c_str());
            }
            if(GetFileAttributesW((Stop+L".clear").c_str())!=INVALID_FILE_ATTRIBUTES){Clear();DeleteFileW((Stop+L".clear").c_str());}
            if(GetFileAttributesW((Stop+L".pause").c_str())!=INVALID_FILE_ATTRIBUTES){Paused=true;Clear();DeleteFileW((Stop+L".pause").c_str());}
            if(GetFileAttributesW((Stop+L".resume").c_str())!=INVALID_FILE_ATTRIBUTES){Clear();Paused=false;DeleteFileW((Stop+L".resume").c_str());}
            Snapshot();Sleep(500);
        }
        bool requestedStop=GetFileAttributesW(Stop.c_str())!=INVALID_FILE_ATTRIBUTES;
        CloseFs(Instance);Instance=nullptr;Active=false;filler.stop();Clear();
        {std::lock_guard<std::mutex> lock(Mutex);Contexts.clear();Leases.clear();}
        Snapshot();Shutdown();RootLease.reset();BCryptCloseAlgorithmProvider(Algorithm,0);Algorithm=nullptr;
        // The pinned Dokan module remains mapped until process teardown.
        return requestedStop?0:3;
    }catch(DWORD code){fwprintf(stderr,L"Folder worker failed: Win32 %lu\n",code);}
    catch(const std::exception& e){fprintf(stderr,"Folder worker failed: %s\n",e.what());}
    if(Instance && CloseFs)CloseFs(Instance);
    return 1;
}
