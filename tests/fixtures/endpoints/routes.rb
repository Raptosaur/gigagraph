require "sinatra"
require "sinatra/namespace"

get "/ping" do
  "pong"
end

post "/echo" do
  request.body.read
end

namespace "/wiki" do
  get "/pages" do
    "pages"
  end

  post "/pages/:name/rename" do
    "renamed"
  end
end
