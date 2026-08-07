// CDK v1: scoped @aws-cdk/* packages (2019-2023). Shapes are identical to
// v2; only the import evidence differs.
import * as cdk from '@aws-cdk/core';
import * as lambda from '@aws-cdk/aws-lambda';
import * as apigateway from '@aws-cdk/aws-apigateway';
import * as appsync from '@aws-cdk/aws-appsync';
import { HttpApi } from '@aws-cdk/aws-apigatewayv2';
import { HttpLambdaIntegration } from '@aws-cdk/aws-apigatewayv2-integrations';

export class V1Stack extends cdk.Stack {
  constructor(scope: cdk.App, id: string) {
    super(scope, id);

    const fn = new lambda.Function(this, 'V1Fn', {
      runtime: lambda.Runtime.NODEJS_14_X,
      handler: 'index.handler',
      code: lambda.Code.fromAsset('lambda'),
    });

    const api = new apigateway.RestApi(this, 'V1Api');
    const gizmos = api.root.addResource('gizmos');
    gizmos.addMethod('GET', new apigateway.LambdaIntegration(fn));

    const httpApi = new HttpApi(this, 'V1Http');
    httpApi.addRoutes({
      path: '/v1orders',
      integration: new HttpLambdaIntegration('V1Int', fn),
    });

    // v1 AppSync: Schema.fromAsset (renamed SchemaFile in v2) and the
    // options-only createResolver arity (no leading id string).
    const gql = new appsync.GraphqlApi(this, 'V1Gql', {
      name: 'v1',
      schema: appsync.Schema.fromAsset('schema.graphql'),
    });
    const ds = gql.addLambdaDataSource('V1Source', fn);
    ds.createResolver({ typeName: 'Query', fieldName: 'getGizmo' });
  }
}
