from aws_cdk import (
    Stack,
    aws_lambda as _lambda,
    aws_apigateway as apigw,
    aws_apigatewayv2 as apigwv2,
    aws_appsync as appsync,
)
from constructs import Construct


class PetStack(Stack):
    def __init__(self, scope: Construct, construct_id: str) -> None:
        super().__init__(scope, construct_id)

        fn = _lambda.Function(
            self,
            "ApiFn",
            runtime=_lambda.Runtime.PYTHON_3_11,
            handler="app.lambda_handler",
            code=_lambda.Code.from_asset("src"),
        )

        api = apigw.RestApi(self, "Api")
        pets = api.root.add_resource("pets")
        pets.add_method("GET", apigw.LambdaIntegration(fn))
        pet = pets.add_resource("{pet_id}")
        pet.add_method("PUT", apigw.LambdaIntegration(fn))

        apigw.LambdaRestApi(self, "Proxy", handler=fn)

        http_api = apigwv2.HttpApi(self, "HttpApi")
        http_api.add_routes(
            path="/orders",
            methods=[apigwv2.HttpMethod.GET, apigwv2.HttpMethod.POST],
            integration=None,
        )

        gql = appsync.GraphqlApi(self, "Gql", name="pets")
        ds = gql.add_lambda_data_source("PetSource", fn)
        ds.create_resolver(
            "MutationAddPet", type_name="Mutation", field_name="addPet"
        )
        appsync.Resolver(
            self,
            "ListPets",
            api=gql,
            type_name="Query",
            field_name="listPets",
        )
