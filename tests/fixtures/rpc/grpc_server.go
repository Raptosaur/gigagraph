package main

import (
	pb "example.com/gen/greeter"

	"google.golang.org/grpc"
)

func main() {
	s := grpc.NewServer()
	pb.RegisterGreeterServer(s, nil)
	_ = s
}
