# DI-shaped fixture: superclass edge, ivar-from-construction, constructed
# local. Ruby has no type annotations — Klass.new shapes are all we get.
class Base
end

class Service < Base
  def initialize(store)
    @store = store
  end

  def setup
    @store = DbStore.new
    x = DbStore.new
    x.save
    @store.persist
  end
end
