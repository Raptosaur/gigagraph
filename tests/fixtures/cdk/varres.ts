// Variable-tracked API Gateway resource trees + v2 construct shapes
// distilled from real stacks (aws-cdk-examples, cdk-patterns/serverless).
import * as apigw from 'aws-cdk-lib/aws-apigateway';
import * as apigwv2 from 'aws-cdk-lib/aws-apigatewayv2';
import { Stack } from 'aws-cdk-lib';

export class VarResStack extends Stack {
  constructor(scope: any, id: string) {
    super(scope, id);

    // api-cors-lambda-crud-dynamodb shape: resources held in variables.
    const api = new apigw.RestApi(this, 'Api');
    const widgets = api.root.addResource('widgets');
    widgets.addMethod('GET', getAllInteg);
    const widget = widgets.addResource('{wid}');
    widget.addMethod('PATCH', updateInteg);
    // my-widget-service shape: root method while other resources exist.
    api.root.addMethod('HEAD', rootInteg);
    // the-dynamo-streamer shape: chained addResource().addMethod().
    api.root.addResource('orders').addMethod('POST', orderInteg);

    // cognito-api-lambda / the-fat-lambda shape: proxy:false LambdaRestApi
    // with explicit resources — the proxy-all row must be suppressed.
    const lr = new apigw.LambdaRestApi(this, 'lr', { handler: fn, proxy: false });
    const hello = lr.root.addResource('vhello');
    hello.addMethod('GET', helloInteg);

    // api-gateway-parallel-step-functions shape.
    const sfn = new apigw.StepFunctionsRestApi(this, 'sfn', { stateMachine: sm });

    // the-simple-webservice shape: HttpApi with a default integration.
    const httpApi = new apigwv2.HttpApi(this, 'h', {
      defaultIntegration: new SomeIntegration(fn2),
    });

    // WebSocket L2 + L1 shapes (api-websocket-lambda-dynamodb uses the L1).
    const ws = new apigwv2.WebSocketApi(this, 'ws', {
      connectRouteOptions: { integration: wsInteg },
    });
    ws.addRoute('sendmessage', { integration: wsInteg });
    new apigwv2.CfnRoute(this, 'cr', { apiId: 'x', routeKey: '$disconnect' });

    // HttpRoute L2 route key.
    const rk = apigwv2.HttpRouteKey.with('/v2books', apigwv2.HttpMethod.PUT);
    new apigwv2.HttpRoute(this, 'r', { httpApi, routeKey: rk, integration: integ });
  }
}
