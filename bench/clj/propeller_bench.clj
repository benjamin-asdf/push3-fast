;; Workload "count-loop" on Propeller (Clojure): same task with :exec_do_range.
;; usage: clojure -M:propeller propeller_bench.clj <N> <repetitions>
(require '[propeller.push.interpreter :as interp]
         '[propeller.push.state :as state])
(let [[n reps] *command-line-args*
      n (parse-long n) reps (parse-long reps)
      run1 (fn [n] (let [program (list 0 0 (dec n) :exec_do_range (list :integer_pop 1 :integer_add))
                         t (System/nanoTime)
                         st (interp/interpret-program program state/empty-state Integer/MAX_VALUE)]
                     [(/ (- (System/nanoTime) t) 1e9) (first (:integer st))]))]
  (dotimes [_ 3] (run1 (min n 20000)))
  (let [rs (repeatedly reps #(run1 n))
        [best res] (first (sort-by first rs))]
    (println (str "Propeller (Clojure)\tcount-loop\t" n "\t" (format "%.6f" best) "\t" res))))
(shutdown-agents)
