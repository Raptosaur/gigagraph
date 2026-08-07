package mypkg

import (
	"database/sql"
	stdlog "log"
)

// OrderStore is satisfied structurally by SQLOrderStore (no implements
// clause exists in Go, so no @hier edge is expected).
type OrderStore interface {
	Save(id int) error
}

type SQLOrderStore struct {
	db     *sql.DB
	logger *stdlog.Logger
}

func (st *SQLOrderStore) Save(id int) error {
	st.logger.Printf("saving %d", id)
	return nil
}

type Dispatcher struct {
	store OrderStore
	name  string
}

// Process takes the interface as a typed parameter; calls through `store`
// resolve via the locals table (bare receiver, no dots).
func Process(store OrderStore, count int) error {
	for i := 0; i < count; i++ {
		if err := store.Save(i); err != nil {
			return err
		}
	}
	return nil
}

// Rewire uses an explicitly typed var plus a pointer-typed var.
func Rewire() {
	var backup OrderStore
	var d *Dispatcher
	backup.Save(0)
	d.Flush()
}

func (d *Dispatcher) Flush() {
	d.store.Save(99)
}
