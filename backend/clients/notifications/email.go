package notifications

import (
	"fmt"
	"net/smtp"
	"strings"
)

type EmailNotifier struct {
	SMTPHost string
	SMTPPort int
	Username string
	Password string
	From     string
	To       []string
}

func (e *EmailNotifier) Name() string {
	return "email"
}

func (e *EmailNotifier) Notify(event NotificationEvent) error {
	addr := fmt.Sprintf("%s:%d", e.SMTPHost, e.SMTPPort)
	auth := smtp.PlainAuth("", e.Username, e.Password, e.SMTPHost)

	body := strings.Join([]string{
		fmt.Sprintf("From: %s", e.From),
		fmt.Sprintf("To: %s", strings.Join(e.To, ", ")),
		fmt.Sprintf("Subject: %s", event.Title),
		"MIME-Version: 1.0",
		"Content-Type: text/plain; charset=\"utf-8\"",
		"",
		event.Message,
	}, "\r\n")

	return smtp.SendMail(addr, auth, e.From, e.To, []byte(body))
}
