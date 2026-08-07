import * as cdk from 'aws-cdk-lib';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import * as apigateway from 'aws-cdk-lib/aws-apigateway';
import * as appsync from 'aws-cdk-lib/aws-appsync';
import { HttpApi, HttpMethod } from 'aws-cdk-lib/aws-apigatewayv2';
import { HttpLambdaIntegration } from 'aws-cdk-lib/aws-apigatewayv2-integrations';

export class PetStack extends cdk.Stack {
  constructor(scope: cdk.App, id: string) {
    super(scope, id);

    const fn = new lambda.Function(this, 'ApiFn', {
      runtime: lambda.Runtime.NODEJS_18_X,
      handler: 'index.handler',
      code: lambda.Code.fromAsset('lambda'),
    });

    const api = new apigateway.RestApi(this, 'Api');
    const users = api.root.addResource('users');
    users.addMethod('GET', new apigateway.LambdaIntegration(fn));
    const one = users.addResource('{userId}');
    one.addMethod('DELETE', new apigateway.LambdaIntegration(fn));
    api.root
      .resourceForPath('/books/{bookId}')
      .addMethod('POST', new apigateway.LambdaIntegration(fn));

    const httpApi = new HttpApi(this, 'HttpApi');
    httpApi.addRoutes({
      path: '/orders',
      methods: [HttpMethod.GET, HttpMethod.POST],
      integration: new HttpLambdaIntegration('OrdersInt', fn),
    });

    const gql = new appsync.GraphqlApi(this, 'Gql', { name: 'pets' });
    const ds = gql.addLambdaDataSource('PetSource', fn);
    ds.createResolver('QueryGetUser', {
      typeName: 'Query',
      fieldName: 'getUser',
    });

    fn.addFunctionUrl({ authType: lambda.FunctionUrlAuthType.NONE });
  }
}
