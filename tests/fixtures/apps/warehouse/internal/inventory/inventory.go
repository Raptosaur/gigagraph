package inventory

import (
	"errors"
	"fmt"
	"sync"
)

var ErrOutOfStock = errors.New("out of stock")

type Item struct {
	SKU      string
	Name     string
	OnHand   int
	Reserved int
}

func (i *Item) Available() int {
	return i.OnHand - i.Reserved
}

func (i *Item) String() string {
	return fmt.Sprintf("%s (%d available)", i.SKU, i.Available())
}

type Service struct {
	mu    sync.Mutex
	items map[string]*Item
}

func NewService() *Service {
	return &Service{items: make(map[string]*Item)}
}

func (s *Service) Put(item *Item) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.items[item.SKU] = item
}

func (s *Service) Reserve(sku string, qty int) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	item, ok := s.items[sku]
	if !ok || item.Available() < qty {
		return ErrOutOfStock
	}
	item.Reserved += qty
	return nil
}

func (s *Service) Release(sku string, qty int) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if item, ok := s.items[sku]; ok {
		item.Reserved = max(0, item.Reserved-qty)
	}
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}
