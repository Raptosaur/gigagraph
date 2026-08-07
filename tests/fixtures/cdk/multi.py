from aws_cdk import Stack, aws_lambda as _lambda, aws_apigatewayv2 as apigwv2


class MultiStack(Stack):
    """Two lambdas in one file: the file-scope handler heuristic must NOT
    pick either one for the route below."""

    def __init__(self, scope, construct_id) -> None:
        super().__init__(scope, construct_id)

        first = _lambda.Function(
            self,
            "First",
            runtime=_lambda.Runtime.PYTHON_3_11,
            handler="app.lambda_handler",
            code=_lambda.Code.from_asset("src"),
        )
        second = _lambda.Function(
            self,
            "Second",
            runtime=_lambda.Runtime.PYTHON_3_11,
            handler="app.lambda_handler",
            code=_lambda.Code.from_asset("src"),
        )

        api = apigwv2.HttpApi(self, "Api")
        api.add_routes(
            path="/multi",
            methods=[apigwv2.HttpMethod.GET],
            integration=None,
        )
