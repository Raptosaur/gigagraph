Rails.application.routes.draw do
  resources :posts
  resource :account
  get "/dashboard", to: "home#index"
end
