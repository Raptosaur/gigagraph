// Mounted admin router: routes of its own, plus a second-level Mount whose
// target is a composite-literal method call — resolvable only through the
// receiver-segment ~ receiver-type fuzz (Heuristic), and transitively
// prefixed (/admin + /accounts).
package admin

import (
	"net/http"

	"github.com/go-chi/chi/v5"
)

type accountsResource struct{}

type groupsResource struct{}

func (rs accountsResource) routes() chi.Router {
	r := chi.NewRouter()
	r.Get("/", rs.list)
	r.Route("/{accountID}", func(r chi.Router) {
		r.Put("/", rs.update)
	})
	return r
}

func (rs accountsResource) list(w http.ResponseWriter, r *http.Request) {}

func (rs accountsResource) update(w http.ResponseWriter, r *http.Request) {}

func (rs groupsResource) routes() chi.Router {
	r := chi.NewRouter()
	r.Get("/", nil)
	return r
}

func Router() chi.Router {
	r := chi.NewRouter()
	r.Get("/", nil)
	r.Mount("/accounts", accountsResource{}.routes())
	r.Mount("/groups", groupsResource{}.routes())
	return r
}
