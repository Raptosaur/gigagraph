// DI-shaped fixture: typed members (value/pointer/reference/smart-pointer),
// multiple base classes, typed constructor params, and typed locals.
#include <memory>
using std::unique_ptr;

class Store {
public:
  void save() {}
};

struct SB {};
struct SD : SB {
  int tag;
};

class Base {};
class Iface {};

class Service : public Base, public Iface {
public:
  Store plain_;
  Store* ptr_;
  Store& ref_;
  std::unique_ptr<Store> owned_;
  std::shared_ptr<Store> shared_;
  unique_ptr<Store> bare_owned_;
  int count_;

  Service(Store store, Store* pstore, const Store& rstore,
          std::shared_ptr<Store> sp)
      : ref_(*pstore) {}

  void run() {
    Store s;
    Store t = Store();
    auto u = Store();
    auto v = make_store();
    s.save();
    ptr_->save();
  }
};
