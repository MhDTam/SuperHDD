// Super HDD folder worker. Uses the unmodified, installed LGPL Dokan runtime.
// The worker owns one mount and one disposable SSD cache; source writes are durable.
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
struct Entry {Key key{};DWORD valid=0;std::array<unsigned char,32> digest{};bool occupied=false,referenced=false;};
struct Context {Handle file;std::wstring path;Id id{};bool directory=false,stream=false;};
struct Lease {Handle file;std::wstring path;uint64_t tick=0,lastEnd=UINT64_MAX,sequential=0;};
static std::mutex Mutex;
static std::wstring Root,Mount,Cache,Ready,Stop,Status;
static Handle CacheHandle,RootLease;
static std::unordered_map<Key,size_t,KeyHash> Index;
static std::vector<Entry> Slots;
static std::unordered_map<Key,Lease,KeyHash> Leases;
static std::unordered_map<Key,std::set<uint64_t>,KeyHash> BlocksByFile;
static std::set<Context*> Contexts;
static size_t Hand=0;
static uint64_t Capacity=0,Hits=0,Misses=0,SsdBytes=0,SourceBytes=0,FillBytes=0,Invalidations=0,Writes=0,ChecksumFailures=0,CacheErrors=0,Tick=0,SequentialBypasses=0,Evictions=0;
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
static bool Paused=false;

static NTSTATUS Error(DWORD code=GetLastError()){fprintf(stderr,"source operation failed: Win32 %lu\n",code);fflush(stderr);return FromError(code);}
static Id Identity(HANDLE h){BY_HANDLE_FILE_INFORMATION info{};if(!GetFileInformationByHandle(h,&info))throw GetLastError();return {info.dwVolumeSerialNumber,(static_cast<uint64_t>(info.nFileIndexHigh)<<32)|info.nFileIndexLow};}
static uint64_t Size(HANDLE h){LARGE_INTEGER size{};if(!GetFileSizeEx(h,&size))throw GetLastError();return static_cast<uint64_t>(size.QuadPart);}
static bool Seek(HANDLE h,uint64_t offset){LARGE_INTEGER p{};p.QuadPart=static_cast<LONGLONG>(offset);return SetFilePointerEx(h,p,nullptr,FILE_BEGIN)!=FALSE;}
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
static void Invalidate(Id id){
    Leases.erase(Key{id,0});
    auto file=BlocksByFile.find(Key{id,0});
    if(file!=BlocksByFile.end()){
        for(uint64_t block:file->second){auto it=Index.find(Key{id,block});if(it!=Index.end()){Slots[it->second].occupied=false;Index.erase(it);++Invalidations;}}
        BlocksByFile.erase(file);
    }
}
static void Forget(const Key& key){
    auto found=Index.find(key);if(found!=Index.end()){Slots[found->second].occupied=false;Index.erase(found);}
    auto file=BlocksByFile.find(Key{key.id,0});if(file!=BlocksByFile.end()){file->second.erase(key.block);if(file->second.empty())BlocksByFile.erase(file);}
}
static void InvalidatePath(const std::wstring& path){
    Handle h(CreateFileW(path.c_str(),FILE_READ_ATTRIBUTES,FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE,nullptr,OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,nullptr));
    if(h)Invalidate(Identity(h.h));
}
static std::array<unsigned char,32> Digest(unsigned char* bytes,DWORD length){
    std::array<unsigned char,32> digest{};
    if(BCryptHash(Algorithm,nullptr,0,bytes,length,digest.data(),static_cast<ULONG>(digest.size()))<0)throw DWORD(ERROR_CRC);
    return digest;
}
static size_t NextSlot(){
    for(;;){size_t slot=Hand;Hand=(Hand+1)%Slots.size();auto& e=Slots[slot];
        if(!e.occupied)return slot;
        if(e.referenced){e.referenced=false;continue;}
        Forget(e.key);++Evictions;return slot;
    }
}
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
        flags&=~(FILE_FLAG_OVERLAPPED|FILE_FLAG_NO_BUFFERING|FILE_FLAG_DELETE_ON_CLOSE);
        flags|=FILE_FLAG_OPEN_REPARSE_POINT;
        if(modifies && !dir)flags|=FILE_FLAG_WRITE_THROUGH;
        auto context=std::make_unique<Context>();
        {Impersonate user(info);context->file=Handle(CreateFileW(path.c_str(),desired,share,nullptr,creation,flags,nullptr));}
        if(!context->file){auto code=GetLastError();fwprintf(stderr,L"open %ls access=0x%lx share=%lu creation=%lu flags=0x%lx: %lu\n",path.c_str(),desired,share,creation,flags,code);return Error(code);}
        BY_HANDLE_FILE_INFORMATION native{};
        if(!GetFileInformationByHandle(context->file.h,&native))return Error();
        if(native.dwFileAttributes&FILE_ATTRIBUTE_REPARSE_POINT)return STATUS_ACCESS_DENIED;
        if(native.dwFileAttributes&FILE_ATTRIBUTE_ENCRYPTED)return STATUS_NOT_SUPPORTED;
        context->path=path;context->directory=dir;context->id=Identity(context->file.h);context->stream=path.find(L':',Root.size())!=std::wstring::npos;
        info->IsDirectory=dir;Contexts.insert(context.get());info->Context=reinterpret_cast<ULONG64>(context.release());
        return STATUS_SUCCESS;
    });
}
static void DOKAN_CALLBACK Cleanup(LPCWSTR,PDOKAN_FILE_INFO info){
    Guard([&]()->NTSTATUS{auto c=Ctx(info);if(c && info->DeletePending){Invalidate(c->id);FILE_DISPOSITION_INFO pending{TRUE};SetFileInformationByHandle(c->file.h,FileDispositionInfo,&pending,sizeof(pending));}return STATUS_SUCCESS;});
}
static void DOKAN_CALLBACK Close(LPCWSTR,PDOKAN_FILE_INFO info){
    Guard([&]()->NTSTATUS{auto c=Ctx(info);if(c){Contexts.erase(c);delete c;info->Context=0;}return STATUS_SUCCESS;});
}
static NTSTATUS DOKAN_CALLBACK Read(LPCWSTR,LPVOID out,DWORD requested,LPDWORD done,LONGLONG offset,PDOKAN_FILE_INFO info){
    *done=0;return Guard([&]()->NTSTATUS{
        auto c=Ctx(info);if(!c || c->directory || offset<0)return STATUS_INVALID_PARAMETER;
        uint64_t size=Size(c->file.h),position=static_cast<uint64_t>(offset);
        if(position>=size || requested==0)return STATUS_SUCCESS;
        DWORD count=static_cast<DWORD>(std::min<uint64_t>(requested,size-position));
        // NTFS streams share the file ID but contain different bytes. Forward
        // them to their own source handle rather than aliasing the main cache.
        if(c->stream || Paused){if(!Seek(c->file.h,position) || !ReadFile(c->file.h,out,count,done,nullptr))return Error();SourceBytes+=*done;return STATUS_SUCCESS;}
        Key leaseKey{c->id,0};auto lease=Leases.find(leaseKey);
        if(lease==Leases.end()){
            Handle h(CreateFileW(c->path.c_str(),GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_DELETE,nullptr,OPEN_EXISTING,FILE_FLAG_NO_BUFFERING|FILE_FLAG_OPEN_REPARSE_POINT,nullptr));
            if(h && Identity(h.h)==c->id){
                if(Leases.size()>=512){auto oldest=std::min_element(Leases.begin(),Leases.end(),[](const auto& a,const auto& b){return a.second.tick<b.second.tick;});Invalidate(oldest->first.id);}
                lease=Leases.emplace(leaseKey,Lease{std::move(h),c->path,++Tick}).first;
            }
        }
        if(lease==Leases.end()){
            Invalidate(c->id);if(!Seek(c->file.h,position) || !ReadFile(c->file.h,out,count,done,nullptr))return Error();SourceBytes+=*done;return STATUS_SUCCESS;
        }
        Buffer buffer;if(!buffer.p)return STATUS_INSUFFICIENT_RESOURCES;
        lease->second.tick=++Tick;
        lease->second.sequential=lease->second.lastEnd==position?lease->second.sequential+count:count;
        lease->second.lastEnd=position+count;bool admit=lease->second.sequential<=8ULL*1024*1024;
        while(*done<count){
            uint64_t p=position+*done,block=p/Block;DWORD within=static_cast<DWORD>(p%Block),valid=0;Key key{c->id,block};
            auto found=Index.find(key);bool hit=false;
            if(found!=Index.end()){
                auto& entry=Slots[found->second];DWORD read=0;
                if(Seek(CacheHandle.h,found->second*static_cast<uint64_t>(Block)) && ReadFile(CacheHandle.h,buffer.p,Block,&read,nullptr) && read==Block){
                    if(Digest(buffer.p,entry.valid)==entry.digest){valid=entry.valid;hit=true;entry.referenced=true;++Hits;SsdBytes+=valid;}
                    else ++ChecksumFailures;
                }else ++CacheErrors;
                if(!hit)Forget(key);
            }
            if(!hit){
                ++Misses;ZeroMemory(buffer.p,Block);
                if(!Seek(lease->second.file.h,block*Block) || !ReadFile(lease->second.file.h,buffer.p,Block,&valid,nullptr))return Error();
                SourceBytes+=valid;if(valid==0)break;
                if(admit){
                    size_t slot=NextSlot();DWORD written=0;
                    if(Seek(CacheHandle.h,slot*static_cast<uint64_t>(Block)) && WriteFile(CacheHandle.h,buffer.p,Block,&written,nullptr) && written==Block){
                        Slots[slot]={key,valid,Digest(buffer.p,valid),true,true};Index[key]=slot;BlocksByFile[leaseKey].insert(block);FillBytes+=valid;
                    }else ++CacheErrors;
                }else ++SequentialBypasses;
            }
            if(valid<=within)break;DWORD n=std::min(valid-within,count-*done);
            memcpy(static_cast<unsigned char*>(out)+*done,buffer.p+within,n);*done+=n;
        }
        return STATUS_SUCCESS;
    });
}
static NTSTATUS DOKAN_CALLBACK Write(LPCWSTR,LPCVOID bytes,DWORD length,LPDWORD done,LONGLONG offset,PDOKAN_FILE_INFO info){
    *done=0;return Guard([&]()->NTSTATUS{
        auto c=Ctx(info);if(!c || c->directory)return STATUS_INVALID_PARAMETER;
        Invalidate(c->id);uint64_t size=Size(c->file.h);
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
    return Guard([&]()->NTSTATUS{auto c=Ctx(info);if(!c || c->directory || length<0)return STATUS_INVALID_PARAMETER;Invalidate(c->id);
        if(!Seek(c->file.h,static_cast<uint64_t>(length)) || !SetEndOfFile(c->file.h) || !FlushFileBuffers(c->file.h))return Error();return STATUS_SUCCESS;});
}
static NTSTATUS DOKAN_CALLBACK Allocation(LPCWSTR name,LONGLONG length,PDOKAN_FILE_INFO info){
    if(length<0)return STATUS_INVALID_PARAMETER;
    return Guard([&]()->NTSTATUS{auto c=Ctx(info);if(!c)return STATUS_INVALID_HANDLE;
        if(static_cast<uint64_t>(length)>=Size(c->file.h))return STATUS_SUCCESS;
        Invalidate(c->id);if(!Seek(c->file.h,static_cast<uint64_t>(length)) || !SetEndOfFile(c->file.h) || !FlushFileBuffers(c->file.h))return Error();(void)name;return STATUS_SUCCESS;});
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
        const auto old=c->path;for(auto context:Contexts)if(_wcsicmp(context->path.c_str(),old.c_str())==0 || (context->path.size()>old.size() && _wcsnicmp(context->path.c_str(),old.c_str(),old.size())==0 && context->path[old.size()]==L'\\'))context->path=path+context->path.substr(old.size());
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
    std::lock_guard<std::mutex> lock(Mutex);uint64_t used=0;for(const auto& e:Slots)if(e.occupied)used+=e.valid;
    auto tmp=Status+L".tmp";std::ofstream stream(fs::path(tmp),std::ios::binary|std::ios::trunc);
    stream<<"{\"active\":"<<(Active?"true":"false")<<",\"paused\":"<<(Paused?"true":"false")<<",\"backend\":\"dokanFolder\",\"capacityBytes\":"<<Capacity<<",\"occupiedBytes\":"<<used<<",\"hits\":"<<Hits<<",\"misses\":"<<Misses<<",\"sourceReadBytes\":"<<SourceBytes<<",\"ssdReadBytes\":"<<SsdBytes<<",\"fillBytes\":"<<FillBytes<<",\"invalidations\":"<<Invalidations<<",\"evictions\":"<<Evictions<<",\"sequentialBypasses\":"<<SequentialBypasses<<",\"durableWrites\":"<<Writes<<",\"checksumFailures\":"<<ChecksumFailures<<",\"cacheErrors\":"<<CacheErrors<<",\"dedicatedRamCacheBytes\":0,\"leasedFileCount\":"<<Leases.size()<<",\"mount\":"<<Json(Mount)<<",\"cachedFiles\":[";
    bool first=true;for(const auto& item:BlocksByFile){auto lease=Leases.find(item.first);if(lease==Leases.end() || !Within(lease->second.path))continue;uint64_t bytes=0;for(uint64_t block:item.second){auto entry=Index.find(Key{item.first.id,block});if(entry!=Index.end())bytes+=Slots[entry->second].valid;}
        if(!first)stream<<',';first=false;stream<<"{\"path\":"<<Json(Mount+lease->second.path.substr(Root.size()))<<",\"residentBytes\":"<<bytes<<"}";}
    stream<<"]}";stream.close();
    MoveFileExW(tmp.c_str(),Status.c_str(),MOVEFILE_REPLACE_EXISTING|MOVEFILE_WRITE_THROUGH);
}
static void Clear(){std::lock_guard<std::mutex> lock(Mutex);Index.clear();BlocksByFile.clear();Leases.clear();for(auto& entry:Slots)entry.occupied=false;Hand=0;}
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
    if(argc!=9 || wcscmp(argv[1],L"run")){fwprintf(stderr,L"Usage: folder-worker run BACKING MOUNT CACHE CAPACITY READY STOP STATUS\n");return 2;}
    try {
        Root=fs::canonical(argv[2]).wstring();Mount=fs::canonical(argv[3]).wstring();Cache=fs::absolute(argv[4]).wstring();Capacity=std::stoull(argv[5]);Ready=argv[6];Stop=argv[7];Status=argv[8];
        if(Root.size()<4 || Mount.size()<4 || Root[0]!=L'D' || Mount[0]!=L'D' || Within(Mount) || _wcsicmp(Root.c_str(),Mount.c_str())==0 || Cache.size()<4 || Cache[0]!=L'M' || _wcsnicmp(Cache.c_str(),Root.c_str(),Root.size())==0 || Capacity<Block || Capacity>25000000000ULL || !fs::is_empty(Mount))throw DWORD(ERROR_INVALID_PARAMETER);
        for(const auto& path:{Root,Mount}){auto attr=GetFileAttributesW(path.c_str());if(attr==INVALID_FILE_ATTRIBUTES || (attr&FILE_ATTRIBUTE_REPARSE_POINT) || !(attr&FILE_ATTRIBUTE_DIRECTORY))throw DWORD(ERROR_ACCESS_DENIED);}
        Capacity-=Capacity%Block;Slots.resize(static_cast<size_t>(Capacity/Block));
        RootLease=Handle(CreateFileW(Root.c_str(),FILE_READ_ATTRIBUTES,FILE_SHARE_READ,nullptr,OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,nullptr));if(!RootLease)throw GetLastError();
        CacheHandle=Handle(CreateFileW(Cache.c_str(),GENERIC_READ|GENERIC_WRITE,0,nullptr,CREATE_NEW,FILE_FLAG_NO_BUFFERING,nullptr));if(!CacheHandle || !Seek(CacheHandle.h,Capacity) || !SetEndOfFile(CacheHandle.h))throw GetLastError();
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
        Active=true;Snapshot();std::ofstream(fs::path(Ready))<<"ready";
        while(Active && GetFileAttributesW(Stop.c_str())==INVALID_FILE_ATTRIBUTES){
            if(GetFileAttributesW((Stop+L".clear").c_str())!=INVALID_FILE_ATTRIBUTES){Clear();DeleteFileW((Stop+L".clear").c_str());}
            if(GetFileAttributesW((Stop+L".pause").c_str())!=INVALID_FILE_ATTRIBUTES){Clear();Paused=true;DeleteFileW((Stop+L".pause").c_str());}
            if(GetFileAttributesW((Stop+L".resume").c_str())!=INVALID_FILE_ATTRIBUTES){Clear();Paused=false;DeleteFileW((Stop+L".resume").c_str());}
            Snapshot();Sleep(200);
        }
        bool requestedStop=GetFileAttributesW(Stop.c_str())!=INVALID_FILE_ATTRIBUTES;
        CloseFs(Instance);Instance=nullptr;Active=false;
        {std::lock_guard<std::mutex> lock(Mutex);for(auto context:Contexts)delete context;Contexts.clear();Leases.clear();}
        Snapshot();Shutdown();CacheHandle.reset();RootLease.reset();BCryptCloseAlgorithmProvider(Algorithm,0);Algorithm=nullptr;
        // The pinned Dokan module remains mapped until process teardown.
        return requestedStop?0:3;
    }catch(DWORD code){fwprintf(stderr,L"Folder worker failed: Win32 %lu\n",code);}
    catch(const std::exception& e){fprintf(stderr,"Folder worker failed: %s\n",e.what());}
    if(Instance && CloseFs)CloseFs(Instance);
    return 1;
}
