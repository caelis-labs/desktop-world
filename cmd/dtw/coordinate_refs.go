package main

import "encoding/json"

func (s *supervisor) resolveApp(ref string) string {
	for i := 0; i < 8 && ref != ""; i++ {
		if app := s.refApps[ref]; app != "" {
			return app
		}
		ref = s.refParents[ref]
	}
	return ""
}

func (s *supervisor) rememberObservation(result, request json.RawMessage) {
	var parsed struct {
		Objects []struct {
			Ref  string `json:"ref"`
			App  string `json:"app"`
			Kind string `json:"kind"`
			Name struct {
				Status string `json:"status"`
				Value  string `json:"value"`
			} `json:"name"`
		} `json:"objects"`
	}
	if json.Unmarshal(result, &parsed) != nil {
		return
	}
	var scope struct {
		Scope struct {
			Refs []string `json:"refs"`
		} `json:"scope"`
	}
	_ = json.Unmarshal(request, &scope)
	for _, object := range parsed.Objects {
		if (object.Kind == "app" || object.Kind == "application") && object.Name.Status == "known" && object.Name.Value != "" {
			s.refApps[object.Ref] = object.Name.Value
		}
	}
	for _, object := range parsed.Objects {
		if object.App != "" && object.App != object.Ref {
			s.refParents[object.Ref] = object.App
		}
		if s.refParents[object.Ref] == "" && len(scope.Scope.Refs) == 1 && object.Ref != scope.Scope.Refs[0] {
			s.refParents[object.Ref] = scope.Scope.Refs[0]
		}
	}
}
