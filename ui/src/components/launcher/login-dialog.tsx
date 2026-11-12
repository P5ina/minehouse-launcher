import { Check, Copy, ExternalLink } from "lucide-react"

import { CipherReveal } from "@/components/react-bits/cipher-reveal"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { useDeviceLogin } from "@/hooks/use-device-login"
import { openUrl } from "@/lib/api"

// Вход через Microsoft по коду устройства.
export function LoginDialog({
  open,
  onOpenChange,
  configured,
  onDone,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  configured: boolean
  onDone: () => Promise<void>
}) {
  const { code, error, copied, copy } = useDeviceLogin({
    active: open,
    configured,
    onDone: async () => {
      onOpenChange(false)
      await onDone()
    },
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Вход через Microsoft</DialogTitle>
          <DialogDescription>
            Откройте страницу входа и введите код. Нужен аккаунт с купленной Minecraft: Java Edition.
          </DialogDescription>
        </DialogHeader>

        <div className="flex h-20 items-center justify-center rounded-lg border bg-muted/40 select-text">
          {code ? (
            <CipherReveal
              text={code.user_code}
              trigger="mount"
              fontSize="2rem"
              fontWeight={600}
              letterSpacing="0.14em"
              color="#fafafa"
              className="font-mono"
            />
          ) : (
            <span className="font-mono text-2xl tracking-[0.14em] text-muted-foreground">
              {error ? "—" : "· · · ·"}
            </span>
          )}
        </div>

        {error && (
          <Alert variant="destructive">
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        )}

        <p className="text-xs text-muted-foreground">
          Нет лицензии? Minecraft: Java Edition входит в PC Game Pass.
        </p>

        <DialogFooter>
          <Button variant="outline" disabled={!code} onClick={copy}>
            {copied ? <Check /> : <Copy />}
            {copied ? "Скопировано" : "Скопировать код"}
          </Button>
          <Button disabled={!code} onClick={() => code && openUrl(code.verification_uri)}>
            <ExternalLink />
            Открыть страницу входа
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
