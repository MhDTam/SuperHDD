#pragma once
// Only the separately built storage benchmark opts in. Normal game workers keep
// their existing buffered source/cache I/O and are never replaced by this tool.
namespace loading {
#ifdef SUPERHDD_STORAGE_BENCHMARK
constexpr DWORD BenchmarkReadFlags=FILE_FLAG_NO_BUFFERING;
constexpr DWORD BenchmarkWriteFlags=FILE_FLAG_WRITE_THROUGH;
constexpr bool BenchmarkStorageIsolation=true;
#define SHD_BENCH_ALIGN alignas(4096)
#else
constexpr DWORD BenchmarkReadFlags=0,BenchmarkWriteFlags=0;
constexpr bool BenchmarkStorageIsolation=false;
#define SHD_BENCH_ALIGN
#endif
}
