# Pre-0.12 (HCL1-flavored) syntax — interpolations quoted everywhere — plus
# pre-4.x provider-era lambda shapes: literal zip filename with
# source_code_hash, no archive_file data source.

variable "stage" {
  default = "prod"
}

resource "aws_lambda_function" "legacy_zip" {
  function_name    = "legacy-zip"
  handler          = "report.lambda_handler"
  runtime          = "python3.6"
  filename         = "build/legacy.zip"
  source_code_hash = "${filebase64sha256("build/legacy.zip")}"
}

resource "aws_lambda_function" "quick_fn" {
  function_name = "quick"
  handler       = "orders.handler"
  runtime       = "nodejs12.x"
  filename      = "build/quick.zip"
}

# Quick-create API: inline route_key + target.
resource "aws_apigatewayv2_api" "quick" {
  name          = "quick-api"
  protocol_type = "HTTP"
  route_key     = "POST /quick"
  target        = "${aws_lambda_function.quick_fn.arn}"
}

resource "aws_apigatewayv2_api" "old_http" {
  name          = "old-http"
  protocol_type = "HTTP"
}

resource "aws_apigatewayv2_integration" "old_int" {
  api_id           = "${aws_apigatewayv2_api.old_http.id}"
  integration_type = "AWS_PROXY"
  integration_uri  = "${aws_lambda_function.quick_fn.invoke_arn}"
}

resource "aws_apigatewayv2_route" "old_route" {
  api_id    = "${aws_apigatewayv2_api.old_http.id}"
  route_key = "GET /old"
  target    = "integrations/${aws_apigatewayv2_integration.old_int.id}"
}
