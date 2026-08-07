package mypkg

import (
	"fmt"
	"net"
	"sync"

	stdlog "log"

	_ "net/http/pprof"
)

// Server accepts TCP connections and answers line-based requests.
type Server struct {
	addr     string
	listener net.Listener
	mu       sync.Mutex
	handled  int
}

// NewServer is the package constructor.
func NewServer(addr string) *Server {
	stdlog.Printf("creating server on %s", addr)
	return &Server{addr: addr}
}

// Run listens and serves until Accept fails.
func (s *Server) Run() error {
	ln, err := net.Listen("tcp", s.addr)
	if err != nil {
		return fmt.Errorf("listen on %s: %w", s.addr, err)
	}
	s.listener = ln
	defer s.Close()

	go func() {
		stdlog.Println("serving on", s.addr)
	}()

	for {
		conn, err := ln.Accept()
		if err != nil {
			return err
		}
		go s.handle(conn)
	}
}

func (s *Server) handle(conn net.Conn) {
	defer conn.Close()

	s.mu.Lock()
	s.handled++
	s.mu.Unlock()

	line, err := ReadLine(conn)
	if err != nil {
		stdlog.Println("read:", err)
		return
	}

	switch Classify(line) {
	case "ping":
		fmt.Fprintln(conn, "pong")
	case "quit":
		return
	default:
		fmt.Fprintln(conn, Normalize(line))
	}
}

// Close shuts the listener down; a method value of it is also used below.
func (s *Server) Close() error {
	if s.listener != nil {
		return s.listener.Close()
	}
	return nil
}

// Shutdown demonstrates invoking a method value.
func (s *Server) Shutdown() error {
	closer := s.Close
	return closer()
}
