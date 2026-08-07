from aws_cdk import core
from aws_cdk import aws_apigatewayv2 as apigwv2
from aws_cdk import aws_appsync as appsync
from aws_cdk import aws_lambda as _lambda


class LegacyL1Stack(core.Stack):
    """CDK v1-era L1 escape hatches: Cfn* constructs with template-shaped
    kwargs. Exactly one CfnFunction, so the file-scope single-lambda
    binding may borrow it for the routes below."""

    def __init__(self, scope, construct_id):
        super().__init__(scope, construct_id)

        fn = _lambda.CfnFunction(
            self,
            "LegacyFn",
            handler="app.lambda_handler",
            runtime="python3.9",
        )
        apigwv2.CfnRoute(
            self,
            "LegacyRoute",
            api_id="api123",
            route_key="GET /legacyitems",
            target="integrations/int123",
        )
        appsync.CfnResolver(
            self,
            "LegacyResolver",
            api_id="gql123",
            type_name="Query",
            field_name="legacyPets",
        )
