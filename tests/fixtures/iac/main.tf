data "archive_file" "handlers_zip" {
  type        = "zip"
  source_dir  = "${path.module}/src/handlers"
  output_path = "${path.module}/build/handlers.zip"
}

resource "aws_lambda_function" "orders" {
  function_name = "orders"
  handler       = "orders.handler"
  runtime       = "nodejs20.x"
  filename      = data.archive_file.handlers_zip.output_path
}

resource "aws_lambda_function" "report_fn" {
  function_name = "report"
  handler       = "report.lambda_handler"
  runtime       = "python3.12"
  filename      = data.archive_file.handlers_zip.output_path
}

resource "aws_lambda_function" "cron" {
  function_name = "cron"
  handler       = "users.create"
  runtime       = "nodejs20.x"
  filename      = data.archive_file.handlers_zip.output_path
}

resource "aws_apigatewayv2_api" "http" {
  name          = "http-api"
  protocol_type = "HTTP"
}

resource "aws_apigatewayv2_integration" "orders_int" {
  api_id           = aws_apigatewayv2_api.http.id
  integration_type = "AWS_PROXY"
  integration_uri  = aws_lambda_function.orders.invoke_arn
}

resource "aws_apigatewayv2_route" "put_order" {
  api_id    = aws_apigatewayv2_api.http.id
  route_key = "PUT /orders/{id}"
  target    = "integrations/${aws_apigatewayv2_integration.orders_int.id}"
}

resource "aws_api_gateway_rest_api" "rest" {
  name = "rest-api"
}

resource "aws_api_gateway_resource" "reports" {
  rest_api_id = aws_api_gateway_rest_api.rest.id
  parent_id   = aws_api_gateway_rest_api.rest.root_resource_id
  path_part   = "reports"
}

resource "aws_api_gateway_resource" "report_id" {
  rest_api_id = aws_api_gateway_rest_api.rest.id
  parent_id   = aws_api_gateway_resource.reports.id
  path_part   = "{reportId}"
}

resource "aws_api_gateway_method" "get_report" {
  rest_api_id   = aws_api_gateway_rest_api.rest.id
  resource_id   = aws_api_gateway_resource.report_id.id
  http_method   = "GET"
  authorization = "NONE"
}

resource "aws_api_gateway_integration" "get_report_int" {
  rest_api_id             = aws_api_gateway_rest_api.rest.id
  resource_id             = aws_api_gateway_resource.report_id.id
  http_method             = aws_api_gateway_method.get_report.http_method
  integration_http_method = "POST"
  type                    = "AWS_PROXY"
  uri                     = aws_lambda_function.report_fn.invoke_arn
}

resource "aws_appsync_graphql_api" "gql" {
  name                = "gql-api"
  authentication_type = "API_KEY"
}

resource "aws_appsync_datasource" "orders_ds" {
  api_id = aws_appsync_graphql_api.gql.id
  name   = "orders_lambda"
  type   = "AWS_LAMBDA"

  lambda_config {
    function_arn = aws_lambda_function.orders.arn
  }
}

resource "aws_appsync_resolver" "get_order" {
  api_id      = aws_appsync_graphql_api.gql.id
  type        = "Query"
  field       = "getOrder"
  data_source = aws_appsync_datasource.orders_ds.name
}

resource "aws_lambda_function_url" "orders_url" {
  function_name      = aws_lambda_function.orders.function_name
  authorization_type = "NONE"
}
