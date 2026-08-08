package inventory

import "testing"

func TestReserveReducesAvailability(t *testing.T) {
	s := newFixture(t)
	if err := s.Reserve("sku-1", 2); err != nil {
		t.Fatalf("reserve: %v", err)
	}
}

func TestReserveRejectsOverdraft(t *testing.T) {
	s := newFixture(t)
	if err := s.Reserve("sku-1", 99); err != ErrOutOfStock {
		t.Fatalf("want ErrOutOfStock, got %v", err)
	}
}

func TestReleaseIsIdempotent(t *testing.T) {
	s := newFixture(t)
	s.Release("sku-1", 5)
	s.Release("sku-1", 5)
}

func BenchmarkReserve(b *testing.B) {
	s := NewService()
	s.Put(&Item{SKU: "sku-1", OnHand: 1 << 20})
	for i := 0; i < b.N; i++ {
		_ = s.Reserve("sku-1", 1)
	}
}

func FuzzReserve(f *testing.F) {
	f.Add("sku-1", 1)
	f.Fuzz(func(t *testing.T, sku string, qty int) {
		_ = NewService().Reserve(sku, qty)
	})
}

func newFixture(t *testing.T) *Service {
	t.Helper()
	s := NewService()
	s.Put(&Item{SKU: "sku-1", OnHand: 4})
	return s
}
