import grpc
from gen import greeter_pb2_grpc


class GreeterServicer(greeter_pb2_grpc.GreeterServicer):
    def SayHello(self, request, context):
        return "hello"


def serve():
    server = grpc.server(None)
    greeter_pb2_grpc.add_GreeterServicer_to_server(GreeterServicer(), server)
    server.start()
