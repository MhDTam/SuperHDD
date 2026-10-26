#define NOMINMAX
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <thread>
#include <vector>

static unsigned char Pattern(uint64_t position){return static_cast<unsigned char>((position*17+position/11+31)%251);}
int wmain(int argc,wchar_t** argv){
    if(argc==3&&std::wstring(argv[1])==L"make"){
        std::ofstream file(std::filesystem::path(argv[2]),std::ios::binary);
        std::vector<unsigned char> bytes(65536);
        for(uint64_t block=0;block<1024;++block){for(size_t i=0;i<bytes.size();++i)bytes[i]=Pattern(block*65536+i);file.write(reinterpret_cast<char*>(bytes.data()),bytes.size());}
        return file?0:1;
    }
    if(argc==3&&std::wstring(argv[1])==L"warm"){
        std::ifstream file(std::filesystem::path(argv[2]),std::ios::binary);std::vector<char> bytes(65536);
        while(file.read(bytes.data(),bytes.size())){}return file.eof()?0:1;
    }
    if(argc!=5)return 2;
    DWORD bytes=std::wcstoul(argv[3],nullptr,10);unsigned threads=std::wcstoul(argv[4],nullptr,10);
    if((bytes!=4096&&bytes!=65536)||threads<1||threads>16)return 2;
    HANDLE file=CreateFileW(argv[2],GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE,nullptr,OPEN_EXISTING,FILE_FLAG_OVERLAPPED|FILE_FLAG_NO_BUFFERING,nullptr);
    if(file==INVALID_HANDLE_VALUE)return 3;
    std::atomic_bool failed{false};std::atomic_uint ready{0};std::atomic_bool go{false};std::atomic_uint next{0};
    std::vector<std::vector<double>> latencies(threads);std::vector<std::thread> workers;
    for(unsigned t=0;t<threads;++t)workers.emplace_back([&,t]{
        auto buffer=static_cast<unsigned char*>(VirtualAlloc(nullptr,bytes,MEM_COMMIT|MEM_RESERVE,PAGE_READWRITE));HANDLE event=CreateEventW(nullptr,TRUE,FALSE,nullptr);
        if(!buffer||!event)failed=true;
        ++ready;while(!go)std::this_thread::yield();
        if(buffer&&event)for(;;){
            unsigned operation=next++;if(operation>=1024)break;
            uint64_t offset=((static_cast<uint64_t>(operation)*509)%1024)*65536;
            OVERLAPPED request{};request.Offset=static_cast<DWORD>(offset);request.OffsetHigh=static_cast<DWORD>(offset>>32);request.hEvent=event;ResetEvent(event);DWORD done=0;
            auto start=std::chrono::steady_clock::now();BOOL ok=ReadFile(file,buffer,bytes,&done,&request);
            if(!ok&&GetLastError()==ERROR_IO_PENDING)ok=GetOverlappedResult(file,&request,&done,TRUE);
            latencies[t].push_back(std::chrono::duration<double,std::milli>(std::chrono::steady_clock::now()-start).count());
            if(!ok||done!=bytes){failed=true;break;}
            for(DWORD i=0;i<done;++i)if(buffer[i]!=Pattern(offset+i)){failed=true;break;}
        }
        if(event)CloseHandle(event);if(buffer)VirtualFree(buffer,0,MEM_RELEASE);
    });
    while(ready<threads)std::this_thread::yield();auto start=std::chrono::steady_clock::now();go=true;
    for(auto& thread:workers)thread.join();double seconds=std::chrono::duration<double>(std::chrono::steady_clock::now()-start).count();CloseHandle(file);
    std::vector<double> all;for(const auto& list:latencies)all.insert(all.end(),list.begin(),list.end());std::sort(all.begin(),all.end());
    if(all.empty())return 4;
    printf("{\"passed\":%s,\"operations\":%zu,\"threads\":%u,\"requestBytes\":%lu,\"seconds\":%.6f,\"MiBPerSecond\":%.4f,\"p50Ms\":%.4f,\"p95Ms\":%.4f,\"p99Ms\":%.4f,\"maxMs\":%.4f}\n",failed?"false":"true",all.size(),threads,bytes,seconds,all.size()*static_cast<double>(bytes)/1048576/seconds,all[all.size()/2],all[(all.size()-1)*95/100],all[(all.size()-1)*99/100],all.back());
    return failed?1:0;
}
