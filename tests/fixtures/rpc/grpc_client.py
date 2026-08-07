import grpc
from gen import greeter_pb2_grpc


def greet(name):
    channel = grpc.insecure_channel("localhost:50051")
    stub = greeter_pb2_grpc.GreeterStub(channel)
    return stub.SayHello(name)
