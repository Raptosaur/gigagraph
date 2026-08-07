# Grape API (shapes from the Grape README). Deliberately does NOT
# `require 'grape'`: Bundler apps rarely require the gem per file, so the
# detector's evidence is the scoped-superclass hierarchy edge
# (`< Grape::API` -> "implements:grape").
module Acme
  class API < Grape::API
    version 'v1', using: :path
    prefix :api
    format :json

    resource :orders do
      desc 'Return an order.'
      get ':id' do
        find_order(params[:id])
      end

      post do
        create_order(params)
      end

      route_param :id do
        get 'receipt' do
          order_receipt(params[:id])
        end
      end
    end

    namespace :admin do
      get 'stats' do
        stats_payload
      end
    end
  end
end
