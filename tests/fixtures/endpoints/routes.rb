require "sinatra"

get "/ping" do
  "pong"
end

post "/echo" do
  request.body.read
end
