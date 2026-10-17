;;; note-search-test.el --- Tests for note-search.el -*- lexical-binding: t; -*-

;; Run: emacs --batch -L . -L <consult dir> -l note-search-test.el -f ert-run-tests-batch-and-exit

;;; Code:

(require 'ert)
(require 'note-search)

(defmacro note-search-test--with-notes (&rest body)
  "Run BODY with a temporary notes directory bound."
  (declare (indent 0))
  `(let* ((dir (make-temp-file "note-search-test" t))
          (note-search-notes-directory dir))
     (unwind-protect (progn ,@body)
       (delete-directory dir t))))

(defun note-search-test--write (file content)
  (make-directory (file-name-directory file) t)
  (with-temp-file file (insert content)))

;;;; Parsing

(ert-deftest note-search-test-parse-todo-line ()
  (should (equal (note-search--parse-line "\"/n/a b.md\":15 [18:10] do it due: 2026")
                 '("/n/a b.md" 15 "[18:10] do it due: 2026"))))

(ert-deftest note-search-test-parse-note-line ()
  (should (equal (note-search--parse-line "/n/x [y].md [0 todos, 2 links]")
                 '("/n/x [y].md" 1 nil)))
  (should (equal (note-search--parse-line "/n/plain.md") '("/n/plain.md" 1 nil))))

(ert-deftest note-search-test-note-name ()
  (should (equal (note-search--note-name "/a/b/My Note.md") "My Note"))
  (should (equal (note-search--note-name "x.markdown") "x")))

;;;; Templates and paths

(ert-deftest note-search-test-render-template ()
  (let ((tpl (make-temp-file "tpl" nil ".md"
                             "{{title}}|{{name}}|{{type}}|{{ref}}|{{refname}}|{{today}}|{{yesterday}}|{{tomorrow}}|{{month}}|{{year}}|{{date}}|100%")))
    (unwind-protect
        (should (equal (note-search-render-template
                        tpl '(:title "T" :name "n" :type "person" :ref "r_x" :refname "r x")
                        (encode-time (list 0 0 12 1 3 2026)))
                       "T|n|person|r_x|r x|2026-03-01|2026-02-28|2026-03-02|March|2026|Sunday, March 01, 2026|100%"))
      (delete-file tpl))
    (should-not (note-search-render-template "/nonexistent/tpl.md" nil))))

(ert-deftest note-search-test-daily-note-path ()
  (note-search-test--with-notes
    (note-search-test--write (expand-file-name "templates/daily.md" dir) "# {{title}}\n{{cursor}}\n")
    (let ((buf nil))
      (unwind-protect
          (progn
            (note-search-note "daily")
            (setq buf (current-buffer))
            (should (equal buffer-file-name
                           (expand-file-name
                            (format "daily/%s/%s.md"
                                    (note-search--date "%Y/%b")
                                    (note-search--date "%Y-%m-%d"))
                            dir)))
            (should (equal (buffer-string)
                           (format "# %s\n \n" (note-search--date "%Y-%m-%d")))))
        (when buf (with-current-buffer buf (set-buffer-modified-p nil)) (kill-buffer buf))))))

(ert-deftest note-search-test-entity-note-path ()
  (note-search-test--with-notes
    (note-search-test--write (expand-file-name "templates/meeting.md" dir)
                             "{{title}} {{name}} {{type}} {{ref}} {{refname}}")
    (let (buf)
      (unwind-protect
          (cl-letf (((symbol-function 'read-string) (lambda (&rest _) "Kick Off")))
            (note-search--open-or-create "person" "Meeting" "adam varga")
            (setq buf (current-buffer))
            (should (equal buffer-file-name
                           (expand-file-name
                            (format "person/adam_varga/Meeting-%s-kick_off.md"
                                    (note-search--date "%Y-%m-%d"))
                            dir)))
            (should (equal (buffer-string) "Kick Off kick_off person adam_varga adam varga")))
        (when buf (kill-buffer buf))))))

(ert-deftest note-search-test-day-note ()
  (note-search-test--with-notes
    (note-search-test--write (expand-file-name "templates/daily.md" dir) "{{title}} {{date}}")
    (let (buf)
      (unwind-protect
          (progn
            (note-search-day-note "2025-12-24")
            (setq buf (current-buffer))
            (should (equal buffer-file-name (expand-file-name "daily/2025/Dec/2025-12-24.md" dir)))
            (should (equal (buffer-string) "2025-12-24 Wednesday, December 24, 2025")))
        (when buf (kill-buffer buf))))
    (should-error (note-search-day-note "2025-13-01") :type 'user-error)
    (should-error (note-search-day-note "24.12.2025") :type 'user-error)))

;;;; Pickers

(ert-deftest note-search-test-find-by-attributes ()
  "Several attribute=value filters are combined into one notes search."
  (let ((pairs (list '("type" . "project") '("status" . "active")))
        (answers (list t nil))
        searched)
    (cl-letf (((symbol-function 'note-search--read-attribute-value)
               (lambda () (pop pairs)))
              ((symbol-function 'y-or-n-p) (lambda (&rest _) (pop answers)))
              ((symbol-function 'note-search-search-notes)
               (lambda (&rest args) (setq searched args))))
      (note-search-find-by-attributes))
    (should (equal searched '(:attributes "type=project,status=active")))))

(ert-deftest note-search-test-empty-result-message ()
  "The CLI's \"No matching notes found.\" line is not a result."
  (let ((script (make-temp-file "fake-ns" nil ".sh"
                                "#!/bin/sh\necho 'No matching notes found.'\n")))
    (unwind-protect
        (let ((note-search-command script)
              (picked nil)
              (shown nil))
          (set-file-modes script #o755)
          (should-not (note-search--lines "notes" "--links" "x"))
          (cl-letf (((symbol-function 'note-search--pick)
                     (lambda (&rest _) (setq picked t)))
                    ((symbol-function 'message)
                     (lambda (fmt &rest args) (setq shown (apply #'format fmt args)))))
            (note-search-search-notes :links "Proj X"))
          (should-not picked)
          (should (equal shown "No notes link to Proj X")))
      (delete-file script))))

(ert-deftest note-search-test-backlink-search-keeps-location ()
  "Selecting a note keeps its file, so the backlink search gets its name."
  (note-search-test--with-notes
    (let ((file (expand-file-name "project/Proj X.md" dir))
          searched)
      (note-search-test--write file "x")
      (cl-letf (((symbol-function 'note-search--read-attribute-value)
                 (lambda () '("type" . "project")))
                ((symbol-function 'note-search--lines)
                 (lambda (&rest _) (list (concat file " [0 todos, 1 links]"))))
                ((symbol-function 'note-search-search-notes)
                 (lambda (&rest args) (setq searched args)))
                ;; The minibuffer returns the chosen text without properties.
                ((symbol-function 'completing-read)
                 (lambda (&rest _) "project/Proj X.md")))
        (note-search-backlink-search))
      (should (equal searched '(:links "Proj X"))))))

;;;; Agenda

(ert-deftest note-search-test-agenda-done ()
  (note-search-test--with-notes
    (let ((file (expand-file-name "project/p.md" dir))
          (date (note-search--date "%Y-%m-%d"))
          (note-search-frontmatter-update nil))
      (note-search-test--write file "# P\n- [ ] first\n- [ ] second\n")
      (with-temp-buffer
        (insert (format "- [ ] second ([p](<%s:3 >))\n" file))
        (goto-char (point-min))
        (note-search-agenda-done)
        (should (equal (buffer-string)
                       (format "- [x] second ([p](<%s:3 >)) ✅ [[%s]]\n" file date))))
      (should (equal (with-temp-buffer (insert-file-contents file) (buffer-string))
                     (format "# P\n- [ ] first\n- [x] second ✅ [[%s]]\n" date)))
      (should-not (find-buffer-visiting file)))))

;;;; Exec blocks

(ert-deftest note-search-test-exec-roundtrip ()
  (with-temp-buffer
    (setq buffer-file-name "/tmp/my note.md")
    (insert "## Sec\n\n```bash exec\n\necho \"# T\"; echo {{note}}\n```\n")
    (note-search-exec-process)
    (should (equal (buffer-string)
                   "## Sec\n\n```bash exec\n\necho \"# T\"; echo {{note}}\n<!-- exec-output -->\n### T\nmy note\n```\n"))
    ;; Second run keeps the existing output.
    (note-search-exec-process)
    (should (= 1 (how-many "exec-output" (point-min) (point-max))))
    (note-search-exec-cleanup)
    (should (equal (buffer-string)
                   "## Sec\n\n```bash exec\n\necho \"# T\"; echo {{note}}\n```\n"))
    (set-buffer-modified-p nil)
    (setq buffer-file-name nil)))

;;;; Frontmatter

(ert-deftest note-search-test-frontmatter ()
  (let ((note-search-frontmatter-format "STAMP"))
    (with-temp-buffer
      (insert "---\ntitle: x\n---\nbody\n")
      (note-search-update-frontmatter)
      (should (equal (buffer-string) "---\ntitle: x\nupdated: STAMP\n---\nbody\n"))
      (let ((note-search-frontmatter-format "NEW"))
        (note-search-update-frontmatter))
      (should (equal (buffer-string) "---\ntitle: x\nupdated: NEW\n---\nbody\n")))
    (with-temp-buffer
      (insert "no frontmatter\n---\n")
      (note-search-update-frontmatter)
      (should (equal (buffer-string) "no frontmatter\n---\n")))
    (with-temp-buffer
      (insert "---\nunclosed\n")
      (note-search-update-frontmatter)
      (should (equal (buffer-string) "---\nunclosed\n")))))

;;;; Links

(ert-deftest note-search-test-replace-links ()
  (cl-letf (((symbol-function 'note-search--note-names)
             (lambda () '("Project X" "x" "adam_varga"))))
    (with-temp-buffer
      (insert "About project-x with Adam Varga, not xylophone. See [[x]] and x.\n")
      (note-search-replace-links)
      (should (equal (buffer-string)
                     "About [[project x]] with [[adam_varga]], not xylophone. See [[x]] and [[x]].\n")))))

(ert-deftest note-search-test-wiki-link-at-point ()
  (with-temp-buffer
    (insert "see [[Foo Bar]] and [[baz]]")
    (goto-char 8)
    (should (equal (note-search--wiki-link-at-point) "Foo Bar"))
    (goto-char 3)
    (should-not (note-search--wiki-link-at-point))))

(ert-deftest note-search-test-url-and-jira-at-point ()
  (with-temp-buffer
    (insert "a [site](<https://x.org/p>) b https://y.org/q) PROJ-12 c")
    (goto-char 5)
    (should (equal (car (note-search--url-at-point)) "https://x.org/p"))
    (goto-char (point-max))
    (goto-char (+ 3 (search-backward "https://y")))
    (should (equal (car (note-search--url-at-point)) "https://y.org/q"))
    (goto-char (+ 2 (search-forward "PROJ")))
    (should (equal (car (note-search--jira-key-at-point)) "PROJ-12"))))

;;;; @ completion

(ert-deftest note-search-test-at-capf ()
  (let ((note-search--names-cache '("augen" "neo4j-migration")))
    (with-temp-buffer
      (insert "hi @neo")
      (let ((res (note-search-at-capf)))
        (should res)
        (should (equal (buffer-substring (nth 0 res) (nth 1 res)) "neo"))
        ;; Simulate accepting a candidate.
        (delete-region (nth 0 res) (nth 1 res))
        (insert "neo4j-migration")
        (funcall (plist-get (nthcdr 3 res) :exit-function) "neo4j-migration" 'finished)
        (should (equal (buffer-string) "hi [[neo4j-migration]]"))))
    (with-temp-buffer
      (insert "user@host")
      (should-not (note-search-at-capf)))))

(ert-deftest note-search-test-insert-dispatch-timeout ()
  (with-temp-buffer
    (cl-letf (((symbol-function 'read-event) (lambda (&rest _) nil)))
      (note-search-insert-dispatch)
      (should (equal (buffer-string) "/")))
    (cl-letf (((symbol-function 'read-event) (lambda (&rest _) ?x)))
      (let ((unread-command-events nil))
        (note-search-insert-dispatch)
        (should (equal unread-command-events '(?x)))))))

;;;; Quick capture

(ert-deftest note-search-test-capture-saves-via-cli ()
  (let (sent)
    (cl-letf (((symbol-function 'make-process)
               (lambda (&rest args) (setq sent (plist-get args :command)) nil))
              ((symbol-function 'pop-to-buffer) (lambda (buf &rest _) (set-buffer buf))))
      (note-search-quick-capture)
      (let ((buf (current-buffer)))
        (goto-char (point-max))
        (insert "Idea\nmore")
        (save-buffer)
        (should (equal sent (list note-search-command "create-note" "-t" "daily" "### Idea\nmore")))
        (should-not (file-exists-p buffer-file-name))
        (with-current-buffer buf (set-buffer-modified-p nil))
        (kill-buffer buf)))))

;;; note-search-test.el ends here
