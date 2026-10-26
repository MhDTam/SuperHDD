#pragma once
#include <list>
#include <unordered_map>
#include <cstdint>
namespace loading {
// A bounded second-touch filter. Only recurring blocks in a recent dense I/O
// burst are eligible. A one-pass scan and ordinary sparse gameplay I/O do not
// fill the SSD. This is a heuristic, not a detector of game loading screens.
template<class Key,class Hash> class Admission {
 struct Seen {uint64_t at;typename std::list<Key>::iterator place;};
 std::list<Key> order_;std::unordered_map<Key,Seen,Hash> seen_;
 uint64_t windowAt_=0,windowBytes_=0,previousBytes_=0;
public:
 void activity(uint64_t now,uint64_t bytes){if(now-windowAt_>=500){previousBytes_=now-windowAt_<1000?windowBytes_:0;windowAt_=now;windowBytes_=0;}windowBytes_+=bytes;}
 bool accept(const Key& key,uint64_t now){
  auto found=seen_.find(key);bool reused=false;
  if(found!=seen_.end()){
   uint64_t age=now-found->second.at;reused=age>=500&&age<=15*60*1000;
   // Do not let subreads of the same block continually reset its first touch.
   if(age>=500){found->second.at=now;order_.splice(order_.end(),order_,found->second.place);}
  }else{if(seen_.size()>=65536){seen_.erase(order_.front());order_.pop_front();}order_.push_back(key);seen_.emplace(key,Seen{now,std::prev(order_.end())});}
  return reused&&windowBytes_+previousBytes_>=1024*1024;
 }
 void clear(){seen_.clear();order_.clear();windowAt_=windowBytes_=previousBytes_=0;}
 size_t size()const{return seen_.size();}
};
}
