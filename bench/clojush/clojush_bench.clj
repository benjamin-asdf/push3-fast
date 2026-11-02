;; Workload "count-loop" on Clojush: add 1 to a counter N times with exec_do*range.
;; usage (inside a Clojush checkout): lein run -m clojush-bench <N> <repetitions>
;; copy this file to <clojush>/src/clojush_bench.clj
(ns clojush-bench
  (:require [clojush.pushstate :as ps]
            [clojush.interpreter :as interp]
            [clojush.globals :as g]
            clojush.instructions.common clojush.instructions.numbers clojush.instructions.code
            [clojush.ns :as cns]))

(defn run1 [n]
  (let [program (list 0 0 (dec n) 'exec_do*range (list 'integer_pop 1 'integer_add))
        t (System/nanoTime)
        st (binding [g/global-max-points 1000000000 g/global-evalpush-limit 2000000000]
             (interp/run-push program (ps/make-push-state)))]
    [(/ (- (System/nanoTime) t) 1e9) (first (:integer st))]))

(defn -main [n reps]
  (let [n (parse-long n) reps (parse-long reps)]
    (dotimes [_ 3] (run1 (min n 20000)))
    (let [[best res] (first (sort-by first (repeatedly reps #(run1 n))))]
      (println (str "Clojush 3.32 (Clojure)\tcount-loop\t" n "\t" (format "%.6f" best) "\t" res))))
  (shutdown-agents))
