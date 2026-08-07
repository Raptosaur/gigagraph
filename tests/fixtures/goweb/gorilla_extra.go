// Sibling file of gorilla_sub.go: the handler resolved cross-file within the
// same package.
package main

import "net/http"

func versionHandler(w http.ResponseWriter, r *http.Request) {}
