// chi routing: Route-closure nesting (byte containment), a Group closure
// (no path — organizational only), and cross-file Mount with a
// package-qualified builder call (go-base shape).
package main

import (
	"net/http"

	"github.com/go-chi/chi/v5"

	"example.com/chiweb/admin"
)

func listArticles(w http.ResponseWriter, r *http.Request) {}

func getArticle(w http.ResponseWriter, r *http.Request) {}

func createArticle(w http.ResponseWriter, r *http.Request) {}

func main() {
	r := chi.NewRouter()

	r.Get("/healthz", nil)

	r.Route("/articles", func(r chi.Router) {
		r.Get("/", listArticles)
		r.Post("/", createArticle)
		r.Route("/{articleID}", func(r chi.Router) {
			r.Get("/", getArticle)
		})
	})

	r.Group(func(r chi.Router) {
		r.Mount("/admin", admin.Router())
	})

	http.ListenAndServe(":3333", r)
}
