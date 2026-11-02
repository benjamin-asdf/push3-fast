(ns push3-fast-oracle.coverage
  "Which Propeller problems are fully covered by the instructions in <names.txt>?
  Usage: ... coverage <names.txt>. Literals and ERCs (non-keywords) are counted separately."
  (:require
   [benjamin-schwerdtner.problems.propeller.problems :as problems]
   [clojure.string :as string]))

(defn -main [names-file]
  (binding [*err* (java.io.Writer/nullWriter)]
    (let [have (set (map keyword (string/split-lines (slurp names-file))))]
      (doseq [p problems/all-problems]
        (let [alpha (problems/alphabet p)
              inputs (mapcat :inputs (:train (problems/train-and-test p 5 5 0)))
              input-kinds (sort (distinct (map #(.getSimpleName (class %)) (remove #(or (number? %) (boolean? %) (string? %)
                                                                    (vector? %))
                                                              inputs))))
              kws (filter keyword? alpha)
              missing (remove (into have (map #(keyword (name %)) (:input-names p))) kws)
              lits (remove keyword? alpha)]
          (println (format "%-22s %3d instructions, %3d missing, %2d literals/ERCs (types: %s)  %s"
                           (name (:name p)) (count kws) (count missing) (count lits)
                           (string/join "," (sort (distinct (map #(.getSimpleName (class %)) lits))))
                           (cond (seq missing) (str "e.g. " (string/join " " (map name (take 4 missing))))
                                 (seq input-kinds) (str "inputs of type " (string/join "," input-kinds))
                                 :else "COVERED")))))))
  (shutdown-agents))
