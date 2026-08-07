Rails.application.routes.draw do
  resources :posts
  resource :account
  get "/dashboard", to: "home#index"
  root "home#index"

  resources :authors, only: [:index, :show] do
    member do
      get :badges
    end
    collection do
      get :featured
    end
    get :preview, on: :member
    resources :books, only: [:create]
  end

  namespace :admin do
    resources :tools, only: [:index]
    get "metrics", to: "metrics#index"
  end

  scope "/api" do
    get "/uptime", to: "health#show"
  end

  match "archive/import", :to => "archive#import", :via => [:get, :post]
end
