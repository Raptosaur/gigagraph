# Same-file `locals` one-hop expansion in template strings; var.* stays raw.
# route_hint quotes "local_route" so the anchored block-line search is
# exercised against a duplicate substring earlier in the file.
locals {
  base       = "/api"
  route_hint = "local_route"
}

resource "aws_lambda_function" "local_fn" {
  function_name = "local-fn"
  handler       = "users.create"
  runtime       = "nodejs20.x"
}

resource "aws_apigatewayv2_api" "local_http" {
  name          = "local-http"
  protocol_type = "HTTP"
}

resource "aws_apigatewayv2_integration" "local_int" {
  api_id           = aws_apigatewayv2_api.local_http.id
  integration_type = "AWS_PROXY"
  integration_uri  = aws_lambda_function.local_fn.invoke_arn
}

resource "aws_apigatewayv2_route" "local_route" {
  api_id    = aws_apigatewayv2_api.local_http.id
  route_key = "GET ${local.base}/users"
  target    = "integrations/${aws_apigatewayv2_integration.local_int.id}"
}

resource "aws_apigatewayv2_route" "var_route" {
  api_id    = aws_apigatewayv2_api.local_http.id
  route_key = "PUT /v/${var.stage}/users"
  target    = "integrations/${aws_apigatewayv2_integration.local_int.id}"
}
