// Shapes that only became visible once member-form `new ns.Ctor(...)`
// expressions carry their arguments: NodejsFunction entry resolution,
// LambdaRestApi proxy-all, single-member addRoutes methods arrays, L1
// CfnRoute routeKey, and the appsync.Resolver L2 construct.
import * as cdk from 'aws-cdk-lib';
import * as apigateway from 'aws-cdk-lib/aws-apigateway';
import * as apigatewayv2 from 'aws-cdk-lib/aws-apigatewayv2';
import * as appsync from 'aws-cdk-lib/aws-appsync';
import { NodejsFunction } from 'aws-cdk-lib/aws-lambda-nodejs';
import { HttpApi, HttpMethod } from 'aws-cdk-lib/aws-apigatewayv2';
import { HttpLambdaIntegration } from 'aws-cdk-lib/aws-apigatewayv2-integrations';

export class V2Stack extends cdk.Stack {
  constructor(scope: cdk.App, id: string) {
    super(scope, id);

    const fn = new NodejsFunction(this, 'OrdersFn', {
      entry: 'fns/orders.ts',
      handler: 'main',
    });

    new apigateway.LambdaRestApi(this, 'V2Api', { handler: fn });

    const httpApi = new HttpApi(this, 'V2Http');
    httpApi.addRoutes({
      path: '/v2reports',
      methods: [HttpMethod.PATCH],
      integration: new HttpLambdaIntegration('ReportsInt', fn),
    });

    new apigatewayv2.CfnRoute(this, 'ItemsRoute', {
      apiId: httpApi.apiId,
      routeKey: 'GET /v2items',
      target: 'integrations/items-int',
    });

    const gql = new appsync.GraphqlApi(this, 'V2Gql', { name: 'orders' });
    gql.addLambdaDataSource('OrdersSource', fn);
    new appsync.Resolver(this, 'StatusResolver', {
      api: gql,
      typeName: 'Query',
      fieldName: 'orderStatus',
      dataSourceName: 'OrdersSource',
    });
  }
}
