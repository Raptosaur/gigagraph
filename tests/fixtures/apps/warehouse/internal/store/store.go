package store

import (
	"database/sql"
	"time"
)

type Row struct {
	SKU       string
	Quantity  int
	UpdatedAt time.Time
}

type Postgres struct {
	db *sql.DB
}

func Open(dsn string) (*Postgres, error) {
	db, err := sql.Open("postgres", dsn)
	if err != nil {
		return nil, err
	}
	return &Postgres{db: db}, nil
}

func (p *Postgres) Load(sku string) (*Row, error) {
	row := p.db.QueryRow("SELECT sku, quantity, updated_at FROM stock WHERE sku = $1", sku)
	out := &Row{}
	if err := row.Scan(&out.SKU, &out.Quantity, &out.UpdatedAt); err != nil {
		return nil, err
	}
	return out, nil
}

func (p *Postgres) Save(r *Row) error {
	_, err := p.db.Exec("UPDATE stock SET quantity = $1 WHERE sku = $2", r.Quantity, r.SKU)
	return err
}

func (p *Postgres) Close() error { return p.db.Close() }
