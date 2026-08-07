package main

import (
	pb "example.com/gen/greeter"

	"google.golang.org/grpc"
)

func callGreeter(conn *grpc.ClientConn) {
	client := pb.NewGreeterClient(conn)
	_ = client
}
