(ns push3-fast-oracle.inventory
  "Writes the instructions used by the 31 Propeller problems, with their shapes, as EDN lines:
  {:name :integer_+ :in [...] :out ... :problems n}. Usage: ... inventory <out.edn>"
  (:require
   [benjamin-schwerdtner.clj-push3-play.instructions.interface :as instr]
   [benjamin-schwerdtner.problems.propeller.problems :as problems]
   [clojure.java.io :as io*]))

(defn -main [out]
  (binding [*err* (java.io.Writer/nullWriter)]
    (let [counts (frequencies (mapcat #(problems/alphabet %) problems/all-problems))
          rows (for [[k n] (sort-by key counts)
                     :let [i (instr/instruction k)]]
                 (if (map? i)
                   {:name k :in (:push.instruction/in i) :out (:push.instruction/out i) :problems n}
                   {:name k :other true :problems n}))]
      (with-open [w (io*/writer out)]
        (doseq [r rows] (.write w (str (pr-str r) "\n"))))
      (println (count rows) "symbols written")))
  (shutdown-agents))
