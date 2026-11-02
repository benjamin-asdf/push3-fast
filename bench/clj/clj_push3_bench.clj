;; Workload "count-loop" on clj-push3: add 1 to a counter N times with :exec_do*range.
;; usage: clojure -M:clj-push3 clj_push3_bench.clj <label> <N> <repetitions>
(require '[benjamin-schwerdtner.clj-push3-play.interpreter :as push])
(let [[label n reps] *command-line-args*
      n (parse-long n) reps (parse-long reps)
      program (list 0 0 (dec n) :exec_do*range (list :integer_pop 1 :integer_+))
      run1 (fn [n] (let [t (System/nanoTime)
                         st (push/execute (push/setup-state)
                                          (list 0 0 (dec n) :exec_do*range (list :integer_pop 1 :integer_+))
                                          {:max-executions Integer/MAX_VALUE})]
                     [(/ (- (System/nanoTime) t) 1e9) (peek (get-in st [:stacks :push/integer]))]))]
  (dotimes [_ 3] (run1 (min n 20000)))
  (let [rs (repeatedly reps #(run1 n))
        [best res] (first (sort-by first rs))]
    (println (str label "\tcount-loop\t" n "\t" (format "%.6f" best) "\t" res))))
(shutdown-agents)
