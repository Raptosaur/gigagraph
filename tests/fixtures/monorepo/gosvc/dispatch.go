package gosvc

type OrderStore struct {
	count int
}

func (o *OrderStore) Save(name string) {
	o.count++
}

type Dispatcher struct {
	store OrderStore
}

func (d *Dispatcher) Flush(name string) {
	d.store.Save(name)
}
