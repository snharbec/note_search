;;; note-search.el --- Notes, todos and agenda via the note_search CLI -*- lexical-binding: t; -*-

;; Author: Stefan Harbeck
;; Version: 0.1.0
;; Package-Requires: ((emacs "29.1") (consult "1.0"))
;; Keywords: outlines, hypermedia, convenience
;; URL: https://github.com/snharbec/note_search

;;; Commentary:

;; Emacs port of the Neovim plugin in lua/note_search.  It shells out to the
;; `note_search' binary and uses consult for pickers (with live preview).
;;
;; Features:
;; - Typed notes (daily, note, person, project, company, department) created
;;   from templates in NOTE_SEARCH_DIR/templates, plus subtypes (Meeting,
;;   Note, Task) for entity notes.
;; - Searches over the note_search database: attribute -> value -> text,
;;   tags, body text, todos, backlinks, live search, recent notes.
;; - Agenda buffer: jump to a todo, mark it done, agenda for the current note.
;; - Link helpers: insert [[links]] to notes, turn note names in the buffer
;;   into links, turn a URL or JIRA key at point into a downloaded note.
;; - Quick capture into today's daily note.
;; - ```exec blocks: run the block's command and show its output inline.
;; - Keeps an `updated:' timestamp in YAML frontmatter on save.
;; - Typing @name in a markdown buffer completes to [[name]].
;; - In insert state, "/" followed quickly by a key inserts a link (/p
;;   project, /e person, /n any note, ...), like the Neovim insert maps.
;;
;; Setup:
;;   (use-package note-search
;;     :load-path "/path/to/note_search/emacs"
;;     :config (note-search-mode 1))

;;; Code:

(require 'cl-lib)
(require 'subr-x)
(require 'consult)

(declare-function evil-insert-state "evil-states")
(declare-function evil-define-key* "evil-core")
(declare-function markdown-mode "markdown-mode")
(defvar evil-state)

;;;; Configuration

(defgroup note-search nil
  "Notes, todos and agenda via the note_search CLI."
  :group 'tools
  :prefix "note-search-")

(defcustom note-search-command "note_search"
  "The note_search executable."
  :type 'string)

(defcustom note-search-notes-directory nil
  "Notes directory.  nil means $NOTE_SEARCH_DIR or ~/.local/share/notes."
  :type '(choice (const :tag "From NOTE_SEARCH_DIR" nil) directory))

(defcustom note-search-templates-directory nil
  "Templates directory.  nil means the templates/ folder in the notes directory."
  :type '(choice (const :tag "Notes directory/templates" nil) directory))

(defcustom note-search-database nil
  "Database file passed with -d.  nil lets note_search use $NOTE_SEARCH_DATABASE."
  :type '(choice (const :tag "CLI default" nil) file))

(defcustom note-search-find-command "fd"
  "The fd executable used to list note files."
  :type 'string)

(defcustom note-search-date-format "%Y-%m-%d"
  "Date format used for day prefixes in file names."
  :type 'string)

(defcustom note-search-suffix ".md"
  "File name suffix of notes."
  :type 'string)

(defcustom note-search-types
  '(("daily" :dir "daily" :subdir "%Y/%b" :template "daily.md" :day-prefix t)
    ("note" :dir "note" :template "note.md" :ask-for-name t)
    ("person" :dir "person" :template "person.md" :ask-for-name t :has-subtypes t)
    ("project" :dir "project" :template "project.md" :ask-for-name t :has-subtypes t)
    ("company" :dir "company" :template "company.md" :ask-for-name t :has-subtypes t)
    ("department" :dir "department" :template "department.md" :ask-for-name t
     :has-subtypes t))
  "Note types.  Each entry is (NAME . PLIST).
PLIST keys: :dir (folder in the notes directory), :template, :subdir (date
format for a sub folder), :day-prefix (prefix file names with the date),
:ask-for-name (prompt for a title), :has-subtypes (notes for an entity are
created inside the entity's folder, with a subtype)."
  :type '(alist :key-type string :value-type plist))

(defcustom note-search-subtypes
  '(("Meeting" :template "meeting.md" :add-type t :day-prefix t)
    ("Note" :template "note.md" :add-type t :day-prefix t)
    ("Task" :template "task.md" :add-type t :day-prefix t))
  "Subtypes for notes about an entity.  Each entry is (NAME . PLIST).
PLIST keys: :template, :add-type (prefix file name with the subtype),
:day-prefix (prefix file name with the date)."
  :type '(alist :key-type string :value-type plist))

(defcustom note-search-frontmatter-update t
  "When non-nil, keep an `updated:' line in the YAML frontmatter on save."
  :type 'boolean)

(defcustom note-search-frontmatter-format "%Y-%m-%d %H:%M:%S"
  "Time format of the frontmatter `updated:' value."
  :type 'string)

(defcustom note-search-exec-auto-execute nil
  "When non-nil, run exec blocks when a markdown file is opened."
  :type 'boolean)

(defcustom note-search-exec-output-marker "<!-- exec-output -->"
  "Line that marks generated output inside an exec block."
  :type 'string)

(defcustom note-search-insert-trigger "/"
  "Insert-state key that starts a link insertion (e.g. \"/p\").
Read when the package is loaded."
  :type 'string)

(defcustom note-search-insert-timeout 0.6
  "Seconds to wait for the key after `note-search-insert-trigger'."
  :type 'number)

(defcustom note-search-insert-commands
  '((?p . note-search-insert-link-project)
    (?e . note-search-insert-link-person)
    (?c . note-search-insert-link-company)
    (?d . note-search-insert-link-today)
    (?D . note-search-insert-link-department)
    (?t . note-search-insert-link-daily)
    (?f . note-search-insert-file-link)
    (?n . note-search-insert-link)
    (?w . note-search-insert-link-week)
    (?4 . note-search-insert-link-4-weeks)
    (?b . note-search-insert-block))
  "Keys after `note-search-insert-trigger' and the commands they run."
  :type '(alist :key-type character :value-type function))

;;;; Paths and small helpers

(defun note-search-notes-dir ()
  "Return the notes directory, with a trailing slash."
  (file-name-as-directory
   (expand-file-name (or note-search-notes-directory
                         (getenv "NOTE_SEARCH_DIR")
                         "~/.local/share/notes"))))

(defun note-search-templates-dir ()
  "Return the templates directory."
  (expand-file-name (or note-search-templates-directory
                        (expand-file-name "templates" (note-search-notes-dir)))))

(defun note-search--date (format &optional time)
  "Format TIME with FORMAT using English month and day names."
  (let ((system-time-locale "C"))
    (format-time-string format time)))

(defun note-search--type (name)
  "Return the plist of note type NAME."
  (or (cdr (assoc name note-search-types))
      (user-error "Unknown type: %s" name)))

(defun note-search--type-dir (name)
  "Return the folder of note type NAME."
  (expand-file-name (plist-get (note-search--type name) :dir) (note-search-notes-dir)))

(defun note-search--note-name (file)
  "Return FILE's name without directory and markdown extension."
  (replace-regexp-in-string "\\.\\(md\\|markdown\\)\\'" ""
                            (file-name-nondirectory file)))

(defun note-search--markdown-file-p (&optional file)
  "Non-nil if FILE (default: the buffer's file) is a markdown file."
  (when-let* ((f (or file buffer-file-name)))
    (string-match-p "\\.\\(md\\|markdown\\)\\'" f)))

(defun note-search--enter-insert ()
  "Switch to evil insert state when evil is active."
  (when (and (bound-and-true-p evil-local-mode) (fboundp 'evil-insert-state))
    (evil-insert-state)))

(defun note-search--sh-quote (s)
  "Single-quote S for sh, like the Lua plugin does."
  (concat "'" (string-replace "'" "'\\''" s) "'"))

;;;; Running note_search

(defun note-search--base-args ()
  "Global arguments for every note_search call."
  (when note-search-database
    (list "-d" (expand-file-name note-search-database))))

(defun note-search--run (&rest args)
  "Run note_search with ARGS.  Return (EXIT-CODE . OUTPUT), stderr merged."
  (with-temp-buffer
    (let* ((default-directory (note-search-notes-dir))
           (code (apply #'process-file note-search-command nil t nil
                        (append (note-search--base-args) args))))
      (cons code (buffer-string)))))

(defconst note-search--empty-result-re
  (concat "\\`No \\(matching \\(notes\\|todos\\|segments\\) found\\."
          "\\|\\(notes\\|attributes\\) found in database\\."
          "\\|values found for field '.*'\\)\\'")
  "Lines note_search prints on stdout when nothing was found.")

(defun note-search--lines (&rest args)
  "Run note_search with ARGS and return stdout lines.
Return nil on failure or when note_search reports that nothing was found."
  (with-temp-buffer
    (let* ((default-directory (note-search-notes-dir))
           (err (make-temp-file "note-search-err"))
           (code (apply #'process-file note-search-command nil (list t err) nil
                        (append (note-search--base-args) args))))
      (unwind-protect
          (if (eq code 0)
              (seq-remove (lambda (line) (string-match-p note-search--empty-result-re line))
                          (split-string (buffer-string) "[\r\n]+" t))
            (message "note_search failed: %s"
                     (string-trim (with-temp-buffer
                                    (insert-file-contents err)
                                    (buffer-string))))
            nil)
        (delete-file err)))))

(defun note-search--list-values (&rest args)
  "Run note_search with ARGS and return the indented values below the header."
  (let (values)
    (dolist (line (cdr (apply #'note-search--lines args)))
      (when (string-match "\\`[ \t]+\\(.+\\)\\'" line)
        (push (match-string 1 line) values)))
    (nreverse values)))

(defun note-search-tags () "All tags." (note-search--list-values "values" "tag"))
(defun note-search-links () "All links." (note-search--list-values "values" "link"))
(defun note-search-attribute-names () "All attribute names." (note-search--list-values "attributes"))
(defun note-search-attribute-values (attr)
  "All values of attribute ATTR."
  (note-search--list-values "values" (concat "attr:" attr)))

(defun note-search--parse-line (line)
  "Parse a note_search result LINE into (FILE LINE-NUMBER TEXT).
Handles todo lines (\"file\":N text) and note lines (file [N todos, M links])."
  (cond
   ((string-match "\\`\"\\([^\"]+\\)\":\\([0-9]+\\)[ \t]*\\(.*\\)\\'" line)
    (list (match-string 1 line) (string-to-number (match-string 2 line))
          (match-string 3 line)))
   ((or (string-match "\\`\\(.+\\)[ \t]+\\[[0-9]+[ \t]+todos" line)
        (string-match "\\`\\(.+\\)[ \t]+\\[" line))
    (list (match-string 1 line) 1 nil))
   (t (list line 1 nil))))

;;;; Pickers

(defun note-search--candidate (line)
  "Turn a note_search result LINE into a picker candidate."
  (pcase-let* ((`(,file ,lnum ,text) (note-search--parse-line line))
               (rel (file-relative-name file (note-search-notes-dir))))
    (propertize (if text (format "%s:%d  %s" rel lnum text) rel)
                'note-search-loc (cons file lnum))))

(defun note-search--location-state ()
  "Consult state function that previews candidate files at their line."
  (let ((open (consult--temporary-files))
        (preview (consult--jump-preview)))
    (lambda (action cand)
      (unless cand (funcall open))
      (funcall preview action
               (when-let* ((cand)
                           ((eq action 'preview))
                           (loc (get-text-property 0 'note-search-loc cand))
                           ((file-exists-p (car loc))))
                 (consult--marker-from-line-column (funcall open (car loc)) (cdr loc) 0))))))

(defun note-search--visit (loc)
  "Open LOC, a (FILE . LINE) pair."
  (find-file (car loc))
  (goto-char (point-min))
  (forward-line (1- (cdr loc))))

(defun note-search--pick (prompt lines &optional category)
  "Pick one of the note_search result LINES with PROMPT and open it."
  (let* ((default-directory (note-search-notes-dir))
         (cands (mapcar #'note-search--candidate lines))
         (cand (consult--read cands
                              :prompt prompt
                              :category category
                              :sort nil
                              :require-match t
                              :lookup #'consult--lookup-member
                              :state (note-search--location-state))))
    (when-let* ((loc (and cand (get-text-property 0 'note-search-loc cand))))
      (note-search--visit loc))))

(defun note-search--fd (dir &rest args)
  "List files below DIR (relative names) with fd and ARGS."
  (unless (executable-find note-search-find-command)
    (user-error "%s not found" note-search-find-command))
  (unless (file-directory-p dir)
    (user-error "Directory not found: %s" dir))
  (let ((default-directory (file-name-as-directory dir)))
    (apply #'process-lines-ignore-status note-search-find-command
           "--type" "f" "--color" "never" args)))

(defun note-search-exit-input ()
  "Return the typed text, not the selected candidate (create a new note)."
  (interactive)
  (if (fboundp 'vertico-exit-input) (vertico-exit-input) (exit-minibuffer)))

(defvar-keymap note-search-create-map
  :doc "Minibuffer keys when the typed name may create a new note."
  "M-e" #'note-search-exit-input)

(defun note-search--read-file (prompt dir files &optional require-match)
  "Pick one of FILES (relative to DIR) with PROMPT and file preview.
Without REQUIRE-MATCH, M-e returns the typed text."
  (let* ((default-directory (file-name-as-directory dir))
         (state (consult--file-preview)))
    (consult--read files
                   :prompt prompt
                   :category 'file
                   :require-match require-match
                   :keymap (unless require-match note-search-create-map)
                   :state (lambda (action cand)
                            (funcall state action (and cand (member cand files) cand))))))

;;;; Templates

(defun note-search-render-template (file patterns &optional time)
  "Render template FILE with PATTERNS (plist) at TIME.  nil if FILE is missing."
  (when (file-readable-p file)
    (let* ((time (or time (current-time)))
           (day (* 24 60 60))
           (vars `(("{{date}}" . ,(note-search--date "%A, %B %d, %Y" time))
                   ("{{time}}" . ,(format-time-string "%H:%M:%S" time))
                   ("{{today}}" . ,(note-search--date "%Y-%m-%d" time))
                   ("{{yesterday}}" . ,(note-search--date "%Y-%m-%d" (time-subtract time day)))
                   ("{{tomorrow}}" . ,(note-search--date "%Y-%m-%d" (time-add time day)))
                   ("{{month}}" . ,(note-search--date "%B" time))
                   ("{{year}}" . ,(format-time-string "%Y" time))
                   ("{{title}}" . ,(or (plist-get patterns :title) ""))
                   ("{{name}}" . ,(or (plist-get patterns :name) ""))
                   ("{{type}}" . ,(or (plist-get patterns :type) ""))
                   ("{{ref}}" . ,(or (plist-get patterns :ref) ""))
                   ("{{refname}}" . ,(or (plist-get patterns :refname) ""))))
           (content (with-temp-buffer
                      (insert-file-contents file)
                      (buffer-string))))
      (dolist (var vars content)
        (setq content (string-replace (car var) (cdr var) content))))))

(defun note-search--position-cursor ()
  "Replace the first {{cursor}} with a space and put point there."
  (goto-char (point-min))
  (when (search-forward "{{cursor}}" nil t)
    (replace-match " " t t)
    (backward-char)))

;;;; Note types

(defvar note-search--origin nil
  "Plist (:file :lines :lang) of the buffer a typed note was started from.
Used by `note-search-insert-file-link' and `note-search-insert-block'.")

(defun note-search--save-origin ()
  "Remember the current file and the selected lines (or current line)."
  (let* ((beg (if (use-region-p) (region-beginning) (point)))
         (end (if (use-region-p) (region-end) (point)))
         (text (save-excursion
                 (buffer-substring-no-properties
                  (progn (goto-char beg) (line-beginning-position))
                  (progn (goto-char end) (line-end-position))))))
    (setq note-search--origin
          (list :file (and buffer-file-name (abbreviate-file-name buffer-file-name))
                :lines (split-string text "\n")
                :lang (replace-regexp-in-string
                       "\\(-ts\\)?-mode\\'" "" (symbol-name major-mode))))))

(defun note-search--create (type subtype file patterns)
  "Create FILE of TYPE/SUBTYPE from its template with PATTERNS and open it."
  (if (file-exists-p file)
      (find-file file)
    (let* ((tconf (note-search--type type))
           (stconf (cdr (assoc subtype note-search-subtypes)))
           (tpl (expand-file-name (or (plist-get stconf :template)
                                      (plist-get tconf :template))
                                  (note-search-templates-dir)))
           (content (note-search-render-template
                     tpl (plist-put (copy-sequence patterns) :type type))))
      (unless content (user-error "Template not found: %s" tpl))
      (make-directory (file-name-directory file) t)
      (with-temp-file file (insert content))
      (find-file file)
      (note-search--position-cursor)
      (note-search--enter-insert))))

(defun note-search--open-or-create (type &optional subtype element)
  "Open or create a note of TYPE (and SUBTYPE) for the entity ELEMENT."
  (let* ((tconf (note-search--type type))
         (stconf (cdr (assoc subtype note-search-subtypes)))
         (element (or element ""))
         (title ""))
    (when (plist-get tconf :ask-for-name)
      (setq title (read-string (format "Title of %s note for %s: " (or subtype "") element)))
      (when (string-empty-p title) (user-error "No title given")))
    (let* ((filename (downcase (string-replace " " "_" title)))
           (name filename)
           (refname (string-replace "_" " " element))
           (element (string-replace " " "_" element)))
      (when (or (plist-get stconf :day-prefix) (plist-get tconf :day-prefix))
        (let ((date (note-search--date note-search-date-format)))
          (setq filename (concat date (if (string-empty-p filename) "" "-") filename))
          (when (string-empty-p title) (setq title date))))
      (when (plist-get stconf :add-type)
        (setq filename (concat subtype "-" filename)))
      (let* ((dir (note-search--type-dir type))
             (dir (if (string-empty-p element) dir (expand-file-name element dir)))
             (dir (if-let* ((sub (plist-get tconf :subdir)))
                      (expand-file-name (note-search--date sub) dir)
                    dir))
             (file (expand-file-name (concat filename note-search-suffix) dir)))
        (if (file-exists-p file)
            (find-file file)
          (note-search--create type subtype file
                               (list :title title :name name :ref element
                                     :refname refname)))))))

(defun note-search--pick-entity (type)
  "Pick an existing entity of TYPE.  Return its name (lowercase, no suffix)."
  (let* ((dir (note-search--type-dir type))
         (file (note-search--read-file (format "Search for instance of type %s: " type)
                                       dir (note-search--fd dir "--max-depth" "1") t)))
    (downcase (string-remove-suffix note-search-suffix file))))

(defun note-search--create-note (type &optional subtype)
  "Create a note of TYPE; entity types ask for the entity and SUBTYPE first."
  (if (not (plist-get (note-search--type type) :has-subtypes))
      (note-search--open-or-create type)
    (let* ((element (note-search--pick-entity type))
           (subtype (or subtype
                        (completing-read "Choose type: "
                                         (mapcar #'car note-search-subtypes) nil t))))
      (note-search--open-or-create type subtype element))))

;;;###autoload
(defun note-search-note (type &optional subtype)
  "Open or create a note of TYPE (and SUBTYPE).
Interactively, choose the type."
  (interactive (list (completing-read "Choose type: " (mapcar #'car note-search-types) nil t)))
  (note-search--save-origin)
  (note-search--create-note type subtype))

;;;###autoload
(defun note-search-daily () "Open today's daily note." (interactive) (note-search-note "daily"))
;;;###autoload
(defun note-search-person-note () "Create a note for a person." (interactive) (note-search-note "person"))
;;;###autoload
(defun note-search-project-note () "Create a note for a project." (interactive) (note-search-note "project"))
;;;###autoload
(defun note-search-company-note () "Create a note for a company." (interactive) (note-search-note "company"))
;;;###autoload
(defun note-search-department-note () "Create a note for a department." (interactive) (note-search-note "department"))

;;;###autoload
(defun note-search-create-type (type)
  "Open an entity of TYPE, or create it when the typed name does not exist."
  (interactive (list (completing-read "Type: " (mapcar #'car note-search-types) nil t)))
  (let* ((tconf (note-search--type type))
         (dir (note-search--type-dir type))
         (files (note-search--fd dir "--max-depth" "1"))
         (input (note-search--read-file "Search notes (new name: type it, M-e): " dir files)))
    (cond
     ((string-empty-p input))
     ((member input files) (find-file (expand-file-name input dir)))
     ((y-or-n-p (format "Do you want to create %s? " input))
      (let* ((sub (plist-get tconf :subdir))
             (dir (if sub (expand-file-name (note-search--date sub) dir) dir))
             (name (downcase (string-replace " " "_" input)))
             (name (if (plist-get tconf :day-prefix)
                       (concat (note-search--date note-search-date-format) "-" name)
                     name)))
        (note-search--create type nil
                             (expand-file-name (concat name note-search-suffix) dir)
                             (list :title input :name name)))))))

;;;###autoload
(defun note-search-create-project () "Open or create a project." (interactive) (note-search-create-type "project"))
;;;###autoload
(defun note-search-create-person () "Open or create a person." (interactive) (note-search-create-type "person"))
;;;###autoload
(defun note-search-create-company () "Open or create a company." (interactive) (note-search-create-type "company"))
;;;###autoload
(defun note-search-create-department () "Open or create a department." (interactive) (note-search-create-type "department"))

(defun note-search--find-in (dir)
  "Find a file below DIR."
  (consult-fd (file-name-as-directory dir)))

;;;###autoload
(defun note-search-find-note () "Find any note file." (interactive) (note-search--find-in (note-search-notes-dir)))
;;;###autoload
(defun note-search-find-company () "Find a company note." (interactive) (note-search--find-in (note-search--type-dir "company")))
;;;###autoload
(defun note-search-find-project () "Find a project note." (interactive) (note-search--find-in (note-search--type-dir "project")))
;;;###autoload
(defun note-search-find-person () "Find a person note." (interactive) (note-search--find-in (note-search--type-dir "person")))
;;;###autoload
(defun note-search-find-department () "Find a department note." (interactive) (note-search--find-in (note-search--type-dir "department")))
;;;###autoload
(defun note-search-find-daily () "Find a daily note." (interactive) (note-search--find-in (note-search--type-dir "daily")))

;;;###autoload
(defun note-search-day-note (date)
  "Open or create the daily note for DATE (YYYY-MM-DD)."
  (interactive (list (read-string "Enter date (YYYY-MM-DD): " (note-search--date "%Y-%m-%d"))))
  (unless (string-match "\\`\\([0-9]\\{4\\}\\)-\\([0-9]\\{2\\}\\)-\\([0-9]\\{2\\}\\)\\'" date)
    (user-error "Invalid date format.  Use YYYY-MM-DD"))
  (let ((year (string-to-number (match-string 1 date)))
        (month (string-to-number (match-string 2 date)))
        (day (string-to-number (match-string 3 date))))
    (unless (and (<= 1 month 12) (<= 1 day 31))
      (user-error "Invalid date: month/day out of range"))
    (let* ((time (encode-time (list 0 0 12 day month year)))
           (dir (expand-file-name (format "daily/%d/%s" year (note-search--date "%b" time))
                                  (note-search-notes-dir)))
           (file (expand-file-name (concat date ".md") dir)))
      (unless (file-exists-p file)
        (let ((content (or (note-search-render-template
                            (expand-file-name "daily.md" (note-search-templates-dir))
                            (list :title date) time)
                           (format "# %s\n\n---\ncreated: %s\ntype: daily\n---\n\n" date date))))
          (make-directory dir t)
          (with-temp-file file (insert content))
          (message "Created day note: %s" date)))
      (find-file file))))

;;;; Searches

(defvar note-search--last-search nil
  "Plist (:attribute :value :text) of the last interactive search.")

(cl-defun note-search-search-notes (&key tags links attributes text search-body
                                         date-range start-date end-date)
  "Search notes with the given filters and pick one."
  (let* ((args (append (list "notes")
                       (and tags (list "--tags" tags))
                       (and links (list "--links" links))
                       (and attributes (list "--attributes" attributes))
                       (and text (list "--text" text))
                       (and search-body (list "--search-body" search-body))
                       (and date-range (list "--date-range" date-range))
                       (and start-date (list "--start-date" start-date))
                       (and end-date (list "--end-date" end-date))
                       (list "--absolute-path" "--sort" "created")))
         (lines (apply #'note-search--lines args)))
    (if lines
        (note-search--pick "Note Search: " lines 'file)
      (if links
          (message "No notes link to %s" links)
        (message "No notes found")))))

(cl-defun note-search-search-todos (&key priority due-date open closed tags text attributes)
  "Search todos with the given filters and pick one."
  (let* ((args (append (list "todos")
                       (and priority (list "--priority" priority))
                       (and due-date (list "--due-date" due-date))
                       (cond (open (list "--open")) (closed (list "--closed")))
                       (and tags (list "--tags" tags))
                       (and text (list "--text" text))
                       (and attributes (list "--attributes" attributes))
                       (list "--absolute-path")))
         (lines (apply #'note-search--lines args)))
    (if lines
        (note-search--pick "Todo Search: " lines)
      (message "No todos found"))))

(defun note-search--read-attribute-value ()
  "Ask for an attribute, then one of its values.  Return (ATTR . VALUE)."
  (let ((attrs (note-search-attribute-names)))
    (unless attrs (user-error "No attributes found in database"))
    (let* ((attr (completing-read "Select Attribute: " attrs nil t))
           (values (or (note-search-attribute-values attr)
                       (user-error "No values found for attribute: %s" attr))))
      (cons attr (completing-read (format "Select Value for %s: " attr) values nil t)))))

(defun note-search--attribute-text-search (todos)
  "Attribute -> value -> optional text; then search notes, or open TODOS."
  (pcase-let* ((`(,attr . ,value) (note-search--read-attribute-value))
               (text (read-string "Search text (optional, RET to skip): ")))
    (setq note-search--last-search (list :attribute attr :value value :text text))
    (let ((attributes (concat attr "=" value))
          (text (and (not (string-empty-p text)) text)))
      (if todos
          (note-search-search-todos :attributes attributes :text text :open t)
        (note-search-search-notes :attributes attributes :text text)))))

;;;###autoload
(defun note-search-interactive-search ()
  "Search notes: pick attribute, then value, then optional text."
  (interactive)
  (note-search--attribute-text-search nil))

;;;###autoload
(defun note-search-interactive-todo ()
  "Search open todos: pick attribute, then value, then optional text."
  (interactive)
  (note-search--attribute-text-search t))

;;;###autoload
(defun note-search-repeat-search ()
  "Repeat the last interactive search."
  (interactive)
  (let ((attr (plist-get note-search--last-search :attribute))
        (value (plist-get note-search--last-search :value))
        (text (plist-get note-search--last-search :text)))
    (unless (and attr value) (user-error "No previous interactive search to repeat"))
    (let ((text (and text (not (string-empty-p text)) text)))
      (message "Repeating search: %s=%s%s" attr value
               (if text (format " (text: %s)" text) ""))
      (note-search-search-notes :attributes (concat attr "=" value) :text text))))

;;;###autoload
(defun note-search-find-by-attributes ()
  "Find a note by attribute values and open it.
Pick an attribute and a value, optionally more pairs (all must match),
then pick one of the matching notes."
  (interactive)
  (let (pairs)
    (while (progn
             (pcase-let ((`(,attr . ,value) (note-search--read-attribute-value)))
               (push (concat attr "=" value) pairs))
             (y-or-n-p (format "Filter: %s.  Add another attribute? "
                               (string-join (reverse pairs) ", ")))))
    (note-search-search-notes :attributes (string-join (nreverse pairs) ","))))

;;;###autoload
(defun note-search-backlink-search ()
  "Pick attribute, value and a matching note; then show notes linking to it."
  (interactive)
  (pcase-let* ((`(,attr . ,value) (note-search--read-attribute-value))
               (filter (concat attr "=" value))
               (lines (or (note-search--lines "notes" "--attributes" filter "--absolute-path")
                          (user-error "No notes found for %s" filter)))
               (default-directory (note-search-notes-dir))
               (cand (consult--read (mapcar #'note-search--candidate lines)
                                    :prompt (format "Select note (%s): " filter)
                                    :category 'file
                                    :sort nil
                                    :require-match t
                                    :lookup #'consult--lookup-member
                                    :state (note-search--location-state)))
               (loc (or (and cand (get-text-property 0 'note-search-loc cand))
                        (user-error "No note selected"))))
    (note-search-search-notes :links (note-search--note-name (car loc)))))

;;;###autoload
(defun note-search-search-by-tag ()
  "Pick a tag and search notes with it."
  (interactive)
  (let ((tags (or (note-search-tags) (user-error "No tags found in database"))))
    (note-search-search-notes :tags (completing-read "Select Tag: " tags nil t))))

;;;###autoload
(defun note-search-search-body (text)
  "Search notes whose body contains TEXT."
  (interactive "sSearch in body: ")
  (note-search-search-notes :search-body text))

;;;###autoload
(defun note-search-live-search ()
  "Search note titles and frontmatter as you type."
  (interactive)
  (let* ((default-directory (note-search-notes-dir))
         (base (cons note-search-command (note-search--base-args)))
         (cand (consult--read
                (consult--process-collection
                    (lambda (input)
                      (append base '("notes" "--absolute-path" "--list")
                              (unless (string-empty-p input) (list "--text" input))))
                  :min-input 0
                  :transform (consult--async-map #'note-search--candidate))
                :prompt "Note Search: "
                :category 'file
                :sort nil
                :require-match t
                :lookup #'consult--lookup-member
                :state (note-search--location-state))))
    (when-let* ((loc (and cand (get-text-property 0 'note-search-loc cand))))
      (note-search--visit loc))))

(defun note-search--list-args (input)
  "note_search arguments for `note-search-list' with query INPUT."
  (append '("notes" "--absolute-path" "--sort" "modified"
            "--format" "{updated}\t{title}\t{filename}")
          (unless (string-blank-p input) (list "--query" (string-trim input)))))

(defun note-search--list-candidate (line)
  "Turn a \"UPDATED<TAB>TITLE<TAB>FILE\" LINE into a candidate, or nil."
  (when (string-match "\\`\\([^\t]*\\)\t\\([^\t]*\\)\t\\(.+\\)\\'" line)
    (let ((updated (match-string 1 line))
          (title (match-string 2 line))
          (file (match-string 3 line)))
      (propertize (format "%-16s  %s" updated
                          (if (string-blank-p title)
                              (file-relative-name file (note-search-notes-dir))
                            title))
                  'note-search-loc (cons file 1)))))

(defun note-search--list-transform (sink)
  "Async step: turn result lines into candidates and keep them unique.
Several notes can share date and title; completion would merge them,
so each candidate gets an invisible running number (consult's tofu)."
  (let ((n 0))
    (lambda (action)
      (cond
       ((eq action 'flush) (setq n 0) (funcall sink action))
       ((consp action)
        (funcall sink (delq nil (mapcar
                                 (lambda (line)
                                   (when-let* ((cand (note-search--list-candidate line)))
                                     (setq n (1+ n))
                                     (consult--tofu-append cand (% n consult--tofu-range))))
                                 action))))
       (t (funcall sink action))))))

;;;###autoload
(defun note-search-list ()
  "List all notes, newest modified first, and narrow them as you type.
The input is a note_search query: `@name' links to note name, `#tag'
has the tag, a plain word is in the title, body or frontmatter.
All terms must match."
  (interactive)
  (let* ((default-directory (note-search-notes-dir))
         (base (cons note-search-command (note-search--base-args)))
         ;; The whole input goes to note_search.  The default `perl' style
         ;; would treat a leading `#' or `@' as a separator.
         (consult-async-split-style 'none)
         (cand (consult--read
                (consult--process-collection
                    (lambda (input) (append base (note-search--list-args input)))
                  :min-input 0
                  :transform #'note-search--list-transform)
                :prompt "Notes (@link #tag word): "
                :category 'note-search
                :sort nil
                :require-match t
                :lookup #'consult--lookup-member
                :state (note-search--location-state))))
    (when-let* ((loc (and cand (get-text-property 0 'note-search-loc cand))))
      (note-search--visit loc))))

(defun note-search--wiki-link-at-point ()
  "Return the name inside the [[...]] link at point, or nil."
  (let ((pos (point)) (bol (line-beginning-position)) found)
    (save-excursion
      (goto-char bol)
      (while (and (not found) (re-search-forward "\\[\\[\\(.*?\\)\\]\\]" (line-end-position) t))
        (when (and (>= pos (match-beginning 0)) (< pos (match-end 0)))
          (setq found (match-string-no-properties 1)))))
    found))

;;;###autoload
(defun note-search-search-backlinks ()
  "Search notes linking to the [[link]] (or word) at point."
  (interactive)
  (let ((name (or (note-search--wiki-link-at-point)
                  (let ((before (buffer-substring-no-properties
                                 (line-beginning-position)
                                 (min (1+ (point)) (line-end-position)))))
                    (and (string-match "\\([^ \t]+\\)\\'" before) (match-string 1 before))))))
    (when (or (null name) (string-empty-p name))
      (user-error "No link found under cursor"))
    (note-search-search-notes :links (string-remove-suffix ".md" name))))

;;;###autoload
(defun note-search-references ()
  "Search notes linking to the current note."
  (interactive)
  (unless buffer-file-name (user-error "Buffer is not visiting a file"))
  (note-search-search-notes :links (note-search--note-name buffer-file-name)))

;;;###autoload
(defun note-search-search-today () "Notes created today." (interactive)
       (note-search-search-notes :date-range "today"))
;;;###autoload
(defun note-search-search-this-week () "Notes created this week." (interactive)
       (note-search-search-notes :date-range "this_week"))
;;;###autoload
(defun note-search-search-last-4-weeks ()
  "Notes created in the last 4 weeks."
  (interactive)
  (note-search-search-notes
   :start-date (format-time-string "%Y%m%d" (time-subtract nil (* 28 24 60 60)))
   :end-date (format-time-string "%Y%m%d")))

;;;###autoload
(defun note-search-search-recent ()
  "Pick a range (today, this week, last 4 weeks) and search notes."
  (interactive)
  (pcase (completing-read "Recent Notes: " '("Today" "This Week" "Last 4 Weeks") nil t)
    ("Today" (note-search-search-today))
    ("This Week" (note-search-search-this-week))
    ("Last 4 Weeks" (note-search-search-last-4-weeks))))

;;;###autoload
(defun note-search-search-files ()
  "Find a file in the notes directory."
  (interactive)
  (note-search-find-note))

;;;; Replace note names with links

(defun note-search--note-names ()
  "Return all note names from the database."
  (delete-dups
   (mapcar (lambda (line) (string-trim line)) (note-search--lines "list-names"))))

(defun note-search--name-regexp (name)
  "Regexp matching NAME with _, - and space interchangeable."
  (mapconcat (lambda (c) (if (memq c '(?_ ?- ?\s)) "[-_ ]" (regexp-quote (string c))))
             name ""))

(defun note-search--inside-link-p (pos)
  "Non-nil if POS is inside a [[...]] link on its line."
  (save-excursion
    (goto-char pos)
    (let ((open (save-excursion (search-backward "[[" (line-beginning-position) t)))
          (close (save-excursion (search-backward "]]" (line-beginning-position) t))))
      (and open (or (null close) (> open close))))))

;;;###autoload
(defun note-search-replace-links ()
  "Turn mentions of note names in the buffer into [[links]].
Longest names first; `_', `-' and space match each other; case is ignored."
  (interactive)
  (let ((names (or (note-search--note-names) (user-error "No notes found in database")))
        (case-fold-search t)
        (count 0))
    (setq names (sort names (lambda (a b) (> (length a) (length b)))))
    (save-excursion
      (dolist (name names)
        (unless (string-empty-p name)
          (let ((re (note-search--name-regexp name))
                (link (concat "[[" (downcase name) "]]")))
            (goto-char (point-min))
            (while (re-search-forward re nil t)
              (let ((beg (match-beginning 0)) (end (match-end 0)))
                (if (or (and (char-before beg)
                             (string-match-p "[[:alnum:]_-]" (string (char-before beg))))
                        (and (char-after end)
                             (string-match-p "[[:alnum:]_-]" (string (char-after end))))
                        (note-search--inside-link-p beg))
                    (goto-char (1+ beg))
                  (delete-region beg end)
                  (goto-char beg)
                  (insert link)
                  (cl-incf count))))))))
    (message "Replaced %d mention%s with links" count (if (= count 1) "" "s"))))

;;;; Agenda

(defconst note-search--agenda-link-re
  "\\[\\([^]]+\\)\\][ \t]*([ \t]*<\\(.+?\\):\\([0-9]+\\)[ \t]*>)"
  "Agenda todo link: [Name](</path/file.md:LINE >).")

(defun note-search--agenda-location ()
  "Return (FILE . LINE) of the agenda todo link on the current line."
  (let ((line (buffer-substring-no-properties (line-beginning-position) (line-end-position))))
    (unless (string-match note-search--agenda-link-re line)
      (user-error "No file link found on this line"))
    (let ((file (string-trim (match-string 2 line)))
          (lnum (string-to-number (match-string 3 line))))
      (unless (file-exists-p file) (user-error "File not found: %s" file))
      (cons file lnum))))

(defun note-search--show-agenda (buffer-name args)
  "Run note_search agenda with ARGS and show the output in BUFFER-NAME."
  (pcase-let ((`(,_code . ,output) (apply #'note-search--run "agenda" args)))
    (when (string-empty-p (string-trim output))
      (user-error "No agenda content generated"))
    (with-current-buffer (get-buffer-create buffer-name)
      (let ((inhibit-read-only t))
        (erase-buffer)
        (insert output))
      (when (fboundp 'markdown-mode) (markdown-mode))
      (goto-char (point-min))
      (set-buffer-modified-p nil))
    (switch-to-buffer buffer-name)))

;;;###autoload
(defun note-search-agenda (&optional no-summary)
  "Show the agenda in a buffer.  With prefix arg NO-SUMMARY, hide the summary."
  (interactive "P")
  (let ((date (note-search--date "%Y-%m-%d")))
    (note-search--show-agenda (format "Agenda-%s.md" date)
                              (and no-summary '("--no-summary")))
    (message "Agenda generated for %s" date)))

;;;###autoload
(defun note-search-agenda-current (&optional no-summary)
  "Show the agenda for the current note.  Prefix arg NO-SUMMARY hides the summary."
  (interactive "P")
  (unless buffer-file-name (user-error "No file associated with current buffer"))
  (let* ((name (note-search--note-name buffer-file-name))
         (type (save-excursion
                 (goto-char (point-min))
                 (when (looking-at "---\n")
                   (let ((end (save-excursion (re-search-forward "^---$" nil t 2))))
                     (when (and end (re-search-forward "^type:[ \t]*#?\\([^ \t\n]+\\)" end t))
                       (match-string-no-properties 1))))))
         (flag (pcase type ("company" "-C") ("person" "-E") ("department" "-D"))))
    (note-search--show-agenda (format "Agenda-%s-%s.md" name (note-search--date "%Y-%m-%d"))
                              (append (and flag (list flag))
                                      (and no-summary '("--no-summary"))
                                      (list name)))
    (message "Agenda generated for %s (type: %s)" name (or type "project"))))

;;;###autoload
(defun note-search-agenda-jump ()
  "Open the file and line of the agenda todo on the current line."
  (interactive)
  (let ((loc (note-search--agenda-location)))
    (note-search--visit loc)
    (recenter)
    (message "Jumped to %s:%d" (car loc) (cdr loc))))

(defun note-search--mark-done (line date)
  "Return todo LINE marked done on DATE."
  (let ((line (replace-regexp-in-string "\\`- \\[[ \t]*\\]" "- [x]" line t t)))
    (if (string-match-p "[ \t]+✅" line)
        line
      (format "%s ✅ [[%s]]" line date))))

;;;###autoload
(defun note-search-agenda-done ()
  "Mark the agenda todo on the current line as done in its note."
  (interactive)
  (pcase-let* ((`(,file . ,lnum) (note-search--agenda-location))
               (date (note-search--date "%Y-%m-%d"))
               (existing (find-buffer-visiting file))
               (buf (or existing (find-file-noselect file))))
    (unwind-protect
        (with-current-buffer buf
          (save-excursion
            (goto-char (point-min))
            (unless (zerop (forward-line (1- lnum)))
              (user-error "Line number out of range"))
            (let ((line (buffer-substring-no-properties (line-beginning-position)
                                                        (line-end-position))))
              (unless (string-match-p "\\`- \\[ \\]" line)
                (user-error "This line is not an open todo item"))
              (delete-region (line-beginning-position) (line-end-position))
              (insert (note-search--mark-done line date))))
          (save-buffer))
      (unless existing (kill-buffer buf)))
    (let ((inhibit-read-only t)
          (line (buffer-substring-no-properties (line-beginning-position) (line-end-position))))
      (save-excursion
        (delete-region (line-beginning-position) (line-end-position))
        (goto-char (line-beginning-position))
        (insert (note-search--mark-done line date))))
    (message "Marked as done: %s:%d" file lnum)))

;;;; Quick capture

(defvar-keymap note-search-capture-mode-map
  :doc "Keymap for the quick capture buffer."
  "C-c C-c" #'save-buffer
  "C-c C-k" #'note-search-capture-discard)

(define-minor-mode note-search-capture-mode
  "Quick capture buffer.  Save (C-s, :w, C-c C-c) sends it to today's daily note."
  :lighter " Capture")

(defun note-search--close-buffer (buf)
  "Kill BUF and close its window (soon, outside the current command)."
  (run-at-time 0 nil
               (lambda ()
                 (when (buffer-live-p buf)
                   (let ((win (get-buffer-window buf)))
                     (with-current-buffer buf (set-buffer-modified-p nil))
                     (kill-buffer buf)
                     (when (and (window-live-p win) (not (one-window-p)))
                       (delete-window win)))))))

(defun note-search-capture-discard ()
  "Close the capture buffer without saving."
  (interactive)
  (note-search--close-buffer (current-buffer)))

(defun note-search--capture-submit ()
  "Send the capture buffer to `note_search create-note -t daily'."
  (let ((text (string-trim-right (buffer-string))))
    (if (string-match-p "\\`[ \t\n]*\\(###[ \t\n]*\\)?\\'" text)
        (message "Discarded empty note")
      (make-process
       :name "note-search-capture"
       :command (list note-search-command "create-note" "-t" "daily" text)
       :buffer (generate-new-buffer " *note-search-capture-output*")
       :sentinel (lambda (proc _event)
                   (unless (process-live-p proc)
                     (let ((out (with-current-buffer (process-buffer proc) (buffer-string))))
                       (if (zerop (process-exit-status proc))
                           (message "Note captured")
                         (message "create-note failed: %s" (string-trim out)))
                       (kill-buffer (process-buffer proc)))))))
    (note-search--close-buffer (current-buffer))
    t))

;;;###autoload
(defun note-search-quick-capture ()
  "Open a capture buffer; saving it appends the text to today's daily note."
  (interactive)
  (let ((buf (generate-new-buffer "note-search-quick-capture")))
    (with-current-buffer buf
      (when (fboundp 'markdown-mode) (markdown-mode))
      (insert "### ")
      ;; A file name lets every save command (C-s, :w, C-x C-s) work; the
      ;; write hook sends the text instead of writing the file.
      (setq buffer-file-name (expand-file-name "note-search-quick-capture.md"
                                               temporary-file-directory))
      (add-hook 'write-contents-functions #'note-search--capture-submit nil t)
      (note-search-capture-mode 1)
      (set-buffer-modified-p nil))
    (pop-to-buffer buf '(display-buffer-below-selected))
    (goto-char (point-max))
    (note-search--enter-insert)))

;;;; URL / JIRA issue at point -> note link

(defun note-search--url-at-point ()
  "Return (URL BEG END) for the markdown link or bare URL at point."
  (let ((pos (point)) (eol (line-end-position)) found)
    (save-excursion
      (goto-char (line-beginning-position))
      (while (and (not found) (re-search-forward "\\[[^]\n]*\\](\\([^)\n]+\\))" eol t))
        (when (and (>= pos (match-beginning 0)) (< pos (match-end 0)))
          (setq found (list (string-trim (match-string-no-properties 1) "<" ">")
                            (match-beginning 0) (match-end 0)))))
      (goto-char (line-beginning-position))
      (while (and (not found) (re-search-forward "https?://[^] \t\n)]+" eol t))
        (when (and (>= pos (match-beginning 0)) (< pos (match-end 0)))
          (setq found (list (match-string-no-properties 0)
                            (match-beginning 0) (match-end 0))))))
    found))

(defun note-search--jira-key-at-point ()
  "Return (KEY BEG END) for the JIRA issue key at point."
  (save-excursion
    (let* ((beg (progn (skip-chars-backward "A-Za-z0-9-") (point)))
           (end (progn (skip-chars-forward "A-Za-z0-9-") (point)))
           (word (buffer-substring-no-properties beg end))
           (case-fold-search nil))
      (when (string-match-p "\\`[A-Z]+-[0-9]+\\'" word)
        (list word beg end)))))

;;;###autoload
(defun note-search-document-to-link ()
  "Turn the URL or JIRA key at point into a note and replace it with a [[link]]."
  (interactive)
  (let ((dir (directory-file-name (note-search-notes-dir))))
    (if-let* ((url (note-search--url-at-point)))
        (pcase-let ((`(,src ,beg ,end) url))
          (message "Converting URL: %s" src)
          (redisplay)
          (pcase-let ((`(,code . ,out) (note-search--run "convert" src "-o" dir)))
            (unless (eq code 0) (user-error "Failed to convert URL: %s" (string-trim out)))
            (unless (string-match "Successfully created note:[ \t]*\\([^\n]+\\)" out)
              (user-error "Could not parse convert output: %s" (string-trim out)))
            (let* ((file (string-trim (replace-regexp-in-string
                                       "[ \t]+(type:.*" "" (match-string 1 out))))
                   (name (note-search--note-name file)))
              (when (string-empty-p name)
                (user-error "Could not extract note name from path: %s" file))
              (save-excursion
                (delete-region beg end)
                (goto-char beg)
                (insert "[[" (downcase name) "]]"))
              (message "Converted to [[%s]]: %s" name file))))
      (pcase-let ((`(,key ,beg ,end) (or (note-search--jira-key-at-point)
                                          (user-error "No URL or JIRA issue found under cursor"))))
        (message "Fetching JIRA issue: %s" key)
        (redisplay)
        (pcase-let ((`(,code . ,out) (note-search--run "jira-issue" key "-o" dir)))
          (unless (eq code 0) (user-error "Failed to fetch JIRA issue: %s" (string-trim out)))
          (save-excursion
            (delete-region beg end)
            (goto-char beg)
            (insert "[[" key "]]"))
          (message "Converted %s to link and saved to jira/" key))))))

;;;; Browser history

;;;###autoload
(defun note-search-browser-history (date days)
  "Import browser history for DATE and the DAYS before it, then open the note."
  (interactive (list (read-string "Date (YYYY-MM-DD): " (note-search--date "%Y-%m-%d"))
                     (read-number "Days: " 1)))
  (pcase-let ((`(,code . ,out) (note-search--run "browser-history" date
                                                 "-o" (directory-file-name (note-search-notes-dir))
                                                 "-n" (number-to-string days))))
    (unless (eq code 0) (user-error "Failed to import browser history: %s" (string-trim out)))
    (if (string-match "Successfully created browser history note: \\([^\n]+\\)" out)
        (find-file (string-trim (match-string 1 out)))
      (message "Browser history imported: %s" (string-trim out)))))

;;;; Exec blocks

(defconst note-search--exec-start-re "^```\\([[:alnum:]_]*\\)[ \t]*exec"
  "Opening fence of an exec block (```exec, ```bash exec, ...).")

(defun note-search--exec-blocks ()
  "Return exec blocks as plists (:start :end :command-line :command :has-output).
Line numbers are 1-based."
  (save-excursion
    (goto-char (point-min))
    (let ((lnum 0) blocks start cmd-line cmd output)
      (while (not (eobp))
        (cl-incf lnum)
        (let ((line (buffer-substring-no-properties (line-beginning-position)
                                                    (line-end-position))))
          (cond
           ((string-match-p note-search--exec-start-re line)
            (setq start lnum cmd-line nil cmd nil output nil))
           ((and start (string= line "```"))
            (when cmd
              (push (list :start start :end lnum :command-line cmd-line
                          :command cmd :has-output output)
                    blocks))
            (setq start nil))
           ((and start (not cmd) (not (string-match-p "\\`[ \t]*\\'" line)))
            (setq cmd line cmd-line lnum))
           ((and start (string-search note-search-exec-output-marker line))
            (setq output t))))
        (forward-line 1))
      blocks)))

(defun note-search--exec-expand (cmd)
  "Expand {{note}}, {{file}} and {{today}} in CMD."
  (let ((file (or buffer-file-name "")))
    (thread-last cmd
                 (string-replace "{{note}}" (note-search--sh-quote (note-search--note-name file)))
                 (string-replace "{{file}}" (note-search--sh-quote file))
                 (string-replace "{{today}}" (note-search--date "%Y-%m-%d")))))

(defun note-search--heading-level-before (lnum)
  "Level of the last markdown heading before line LNUM, or 0."
  (save-excursion
    (goto-char (point-min))
    (forward-line (1- lnum))
    (if (re-search-backward "^\\(#+\\) " nil t)
        (length (match-string 1))
      0)))

(defun note-search-exec-process ()
  "Run exec blocks that have no output yet and insert their output."
  (interactive)
  (let ((blocks (note-search--exec-blocks)))
    ;; Blocks come last-first, so inserting does not shift later blocks.
    (dolist (block blocks)
      (unless (plist-get block :has-output)
        (let* ((output (string-trim-right
                        (shell-command-to-string
                         (concat (note-search--exec-expand (plist-get block :command)) " 2>&1"))
                        "\n+"))
               (level (note-search--heading-level-before (plist-get block :start)))
               (lines (split-string output "\n"))
               (lines (if (> level 0)
                          (mapcar (lambda (l)
                                    (if (string-match-p "\\`#+ " l)
                                        (concat (make-string level ?#) l)
                                      l))
                                  lines)
                        lines)))
          (save-excursion
            (goto-char (point-min))
            (forward-line (plist-get block :command-line))
            (insert note-search-exec-output-marker "\n")
            (unless (string-empty-p output)
              (insert (string-join lines "\n") "\n"))))))
    blocks))

(defun note-search-exec-cleanup ()
  "Remove generated output from all exec blocks."
  (interactive)
  (save-excursion
    (goto-char (point-min))
    (let (in-block)
      (while (not (eobp))
        (let ((line (buffer-substring-no-properties (line-beginning-position)
                                                    (line-end-position))))
          (cond
           ((string-match-p note-search--exec-start-re line)
            (setq in-block t)
            (forward-line 1))
           ((and in-block (string= line "```"))
            (setq in-block nil)
            (forward-line 1))
           ((and in-block (string-search note-search-exec-output-marker line))
            (let ((beg (line-beginning-position)))
              (while (and (not (eobp))
                          (not (string= (buffer-substring-no-properties
                                         (line-beginning-position) (line-end-position))
                                        "```")))
                (forward-line 1))
              (delete-region beg (point))))
           (t (forward-line 1))))))))

;;;###autoload
(defun note-search-exec-toggle ()
  "Run exec blocks, or remove their output if there is any."
  (interactive)
  (unless (note-search--markdown-file-p) (user-error "Not a markdown file"))
  (let ((blocks (note-search--exec-blocks)))
    (cond
     ((null blocks) (message "No exec blocks found"))
     ((seq-some (lambda (b) (plist-get b :has-output)) blocks)
      (note-search-exec-cleanup)
      (message "Exec output removed"))
     (t (note-search-exec-process)
        (message "Markdown exec blocks processed")))))

;;;; Frontmatter `updated:'

(defun note-search-update-frontmatter ()
  "Set `updated:' in the YAML frontmatter to now.  No-op without frontmatter."
  (save-excursion
    (save-restriction
      (widen)
      (goto-char (point-min))
      (when (looking-at "---$")
        (forward-line 1)
        (let ((body-start (point))
              (line (concat "updated: " (format-time-string note-search-frontmatter-format))))
          (when (re-search-forward "^---$" nil t)
            (let ((close (line-beginning-position)))
              (goto-char body-start)
              (if (re-search-forward "^updated[ \t]*:.*$" close t)
                  (replace-match line t t)
                (goto-char close)
                (insert line "\n")))))))))

;;;; @ completion

(defvar note-search--names-cache nil
  "Note names (lowercase), newest first, for @ completion.")

(defvar note-search--names-process nil)
(defvar note-search--names-timer nil)

(defun note-search-refresh-names ()
  "Rebuild the @ completion cache in the background."
  (interactive)
  (unless (process-live-p note-search--names-process)
    (let ((buf (generate-new-buffer " *note-search-names*"))
          (default-directory (note-search-notes-dir)))
      (setq note-search--names-process
            (make-process
             :name "note-search-names"
             :command (append (list note-search-command) (note-search--base-args)
                              '("notes" "--list" "--sort" "modified" "--absolute-path"))
             :buffer buf
             :stderr (get-buffer-create " *note-search-names-stderr*")
             :noquery t
             :sentinel
             (lambda (proc _event)
               (unless (process-live-p proc)
                 (when (zerop (process-exit-status proc))
                   (let (names)
                     (dolist (line (with-current-buffer buf
                                     (split-string (buffer-string) "\n" t)))
                       (let ((file (car (note-search--parse-line line))))
                         (push (downcase (note-search--note-name (string-trim file))) names)))
                     (setq note-search--names-cache (delete-dups (nreverse names)))))
                 (kill-buffer buf))))))))

(defun note-search--schedule-refresh ()
  "Refresh the name cache shortly (once per burst of saves)."
  (when (timerp note-search--names-timer) (cancel-timer note-search--names-timer))
  (setq note-search--names-timer (run-with-idle-timer 0.5 nil #'note-search-refresh-names)))

(defun note-search--names-table (string pred action)
  "Completion table of note names, keeping newest-first order."
  (if (eq action 'metadata)
      '(metadata (category . note-search-note)
                 (display-sort-function . identity)
                 (cycle-sort-function . identity))
    (complete-with-action action note-search--names-cache string pred)))

(defun note-search-at-capf ()
  "Complete @name to [[name]]."
  (let* ((end (point))
         (beg (save-excursion (skip-chars-backward "[:alnum:]_-") (point))))
    (when (and (eq (char-before beg) ?@)
               (let ((prev (char-before (1- beg))))
                 (or (null prev) (not (string-match-p "[[:alnum:]_-]" (string prev))))))
      (unless note-search--names-cache (note-search-refresh-names))
      (list beg end #'note-search--names-table
            :exclusive 'no
            :company-prefix-length t
            :annotation-function (lambda (_) " note")
            :exit-function
            (lambda (name status)
              (when (eq status 'finished)
                (let ((start (- (point) (length name) 1)))
                  (when (eq (char-after start) ?@)
                    (delete-region start (point))
                    (insert "[[" (downcase name) "]]")))))))))

;;;; Link insertion

(defun note-search--insert-link-from (prompt dir &rest fd-args)
  "Pick a note below DIR (fd FD-ARGS) and insert a [[link]] to it."
  (let* ((files (apply #'note-search--fd dir fd-args))
         (file (note-search--read-file prompt dir files t))
         (text (concat "[[" (downcase (note-search--note-name file)) "]]  ")))
    (insert text)
    (note-search--enter-insert)))

(defun note-search-insert-link-type (type)
  "Insert a link to an entity of TYPE."
  (note-search--insert-link-from (format "Search %s: " type)
                                 (note-search--type-dir type)
                                 "--max-depth" "1"))

;;;###autoload
(defun note-search-insert-link () "Insert a link to any note." (interactive)
       (note-search--insert-link-from "Search note: " (note-search-notes-dir)))
;;;###autoload
(defun note-search-insert-link-project () "Insert a link to a project." (interactive)
       (note-search-insert-link-type "project"))
;;;###autoload
(defun note-search-insert-link-person () "Insert a link to a person." (interactive)
       (note-search-insert-link-type "person"))
;;;###autoload
(defun note-search-insert-link-company () "Insert a link to a company." (interactive)
       (note-search-insert-link-type "company"))
;;;###autoload
(defun note-search-insert-link-department () "Insert a link to a department." (interactive)
       (note-search-insert-link-type "department"))
;;;###autoload
(defun note-search-insert-link-daily () "Insert a link to a daily note." (interactive)
       (note-search-insert-link-type "daily"))

(defun note-search--insert-link-newer (prompt seconds)
  "Insert a link to a note changed in the last SECONDS, picked with PROMPT."
  (note-search--insert-link-from prompt (note-search-notes-dir) "--newer"
                                 (format-time-string "%Y-%m-%d" (time-subtract nil seconds))))

;;;###autoload
(defun note-search-insert-link-today () "Insert a link to a note changed today." (interactive)
       (note-search--insert-link-from "Search notes (today): " (note-search-notes-dir)
                                      "--newer" (format-time-string "%Y-%m-%d")))
;;;###autoload
(defun note-search-insert-link-week () "Insert a link to a note changed this week." (interactive)
       (note-search--insert-link-newer "Search notes (current week): " (* 7 24 60 60)))
;;;###autoload
(defun note-search-insert-link-4-weeks () "Insert a link to a note changed in 4 weeks." (interactive)
       (note-search--insert-link-newer "Search recent notes (last 4 weeks): " (* 28 24 60 60)))

;;;###autoload
(defun note-search-insert-file-link ()
  "Insert a link to the file the last typed note was started from."
  (interactive)
  (let ((file (plist-get note-search--origin :file)))
    (unless file (user-error "No file to link"))
    (insert (format "[%s](%s)  " (file-name-nondirectory file) file))
    (note-search--enter-insert)))

;;;###autoload
(defun note-search-insert-block ()
  "Insert the lines selected when the last typed note was started, as a code block."
  (interactive)
  (let ((lines (plist-get note-search--origin :lines)))
    (if (not lines)
        (message "No selection")
      (insert (format "```%s\n%s\n```\n" (plist-get note-search--origin :lang)
                      (string-join lines "\n")))))
  (note-search--enter-insert))

(defun note-search-insert-dispatch ()
  "Run a link command if the next key comes quickly, else insert the trigger key."
  (interactive)
  (let* ((event (read-event nil nil note-search-insert-timeout))
         (command (and event (alist-get event note-search-insert-commands))))
    (if command
        (call-interactively command)
      (let ((last-command-event (aref note-search-insert-trigger 0)))
        (self-insert-command 1))
      (when event (push event unread-command-events)))))

;;;; Modes

(defvar-keymap note-search-markdown-mode-map
  :doc "Keymap for `note-search-markdown-mode'.")

(if (or (featurep 'evil) (locate-library "evil"))
    (with-eval-after-load 'evil
      (evil-define-key* 'insert note-search-markdown-mode-map
        note-search-insert-trigger #'note-search-insert-dispatch))
  (keymap-set note-search-markdown-mode-map note-search-insert-trigger
              #'note-search-insert-dispatch))

(defun note-search--before-save ()
  "Clean exec output and update frontmatter before saving a note."
  (when (note-search--markdown-file-p)
    (note-search-exec-cleanup)
    (when note-search-frontmatter-update
      (note-search-update-frontmatter))))

(define-minor-mode note-search-markdown-mode
  "Note features in a markdown buffer: @ completion, link keys, save hooks."
  :lighter nil
  (if note-search-markdown-mode
      (progn
        (add-hook 'completion-at-point-functions #'note-search-at-capf nil t)
        (add-hook 'before-save-hook #'note-search--before-save nil t)
        (add-hook 'after-save-hook #'note-search--schedule-refresh nil t)
        (when (and note-search-exec-auto-execute (note-search--markdown-file-p))
          (note-search-exec-process)))
    (remove-hook 'completion-at-point-functions #'note-search-at-capf t)
    (remove-hook 'before-save-hook #'note-search--before-save t)
    (remove-hook 'after-save-hook #'note-search--schedule-refresh t)))

;;;###autoload
(define-minor-mode note-search-mode
  "Enable `note-search-markdown-mode' in every markdown buffer."
  :global t
  :group 'note-search
  (dolist (hook '(markdown-mode-hook gfm-mode-hook))
    (if note-search-mode
        (add-hook hook #'note-search-markdown-mode)
      (remove-hook hook #'note-search-markdown-mode)))
  (dolist (buf (buffer-list))
    (with-current-buffer buf
      (when (derived-mode-p 'markdown-mode)
        (note-search-markdown-mode (if note-search-mode 1 -1)))))
  (when note-search-mode
    (note-search--schedule-refresh)))

(provide 'note-search)
;;; note-search.el ends here
