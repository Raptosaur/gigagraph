require "net/http"

def warm_cache
  Net::HTTP.get(URI("https://status.example.com/ping"))
end
