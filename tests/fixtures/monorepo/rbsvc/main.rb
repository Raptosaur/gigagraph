require "json"
require_relative "helper"

class Checkout
  def receipt(cents)
    label = Format.currency(cents)
    JSON.generate({ total: label })
  end
end
